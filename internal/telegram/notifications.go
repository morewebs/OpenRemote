package telegram

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/approval"
	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

func (b *Bot) NotifyApproval(_ context.Context, chatID int64, app *approval.PendingApproval) {
	if app == nil {
		return
	}
	copy := *app
	b.enqueue("approval:"+app.ID, notification{ChatID: chatID, Approval: &copy})
}
func (b *Bot) NotifyQuestion(_ context.Context, chatID int64, question protocol.QuestionAskedEvent) {
	b.enqueue("question:"+question.QuestionID, notification{ChatID: chatID, Question: &question})
}
func (b *Bot) NotifyArtifact(_ context.Context, chatID int64, artifact protocol.ArtifactUpdatedEvent) {
	ext := strings.ToLower(filepath.Ext(artifact.Path))
	if (ext != ".md" && ext != ".patch") || artifact.Content == "" || len(artifact.Content) > 4*1024*1024 {
		return
	}
	b.enqueue("artifact:"+artifact.SessionID+":"+artifact.Path, notification{ChatID: chatID, Artifact: &artifact})
}
func (b *Bot) enqueue(key string, action notification) {
	b.mu.Lock()
	if !b.running {
		b.mu.Unlock()
		return
	}
	if len(b.actions) >= 512 {
		b.lastError = "Telegram notification backlog is full"
		b.mu.Unlock()
		return
	}
	b.actions[key] = action
	b.mu.Unlock()
	select {
	case b.wake <- struct{}{}:
	default:
	}
}
func (b *Bot) NotifyChatMessage(_ context.Context, chatID int64, msg chat.Message) {
	if msg.Role != protocol.RoleAssistant || msg.Text == "" {
		return
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if !b.running {
		return
	}
	key := msg.SessionID + ":" + msg.ID
	if _, exists := b.pending[key]; !exists && len(b.pending) >= 512 {
		b.lastError = "Telegram message backlog is full"
		return
	}
	if old, ok := b.pending[key]; ok && old.Message.Rev > msg.Rev {
		return
	}
	b.pending[key] = pendingMessage{Message: msg, ChatID: chatID}
	b.generating[msg.SessionID] = msg.Streaming
}

func (b *Bot) outbox(ctx context.Context) {
	flush := time.NewTicker(2 * time.Second)
	defer flush.Stop()
	typing := time.NewTicker(4500 * time.Millisecond)
	defer typing.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-b.wake:
			b.flushActions(ctx)
		case <-flush.C:
			b.flushActions(ctx)
			b.flushMessages(ctx)
			b.mu.Lock()
			for token, action := range b.callbacks {
				if time.Now().After(action.Expires) {
					delete(b.callbacks, token)
				}
			}
			b.mu.Unlock()
		case <-typing.C:
			if b.throttled() {
				continue
			}
			b.mu.RLock()
			var sessions []string
			for session, active := range b.generating {
				if active {
					sessions = append(sessions, session)
				}
			}
			b.mu.RUnlock()
			for _, session := range sessions {
				r := b.routeFor(ctx, session, 0)
				if r.ChatID != 0 {
					_ = b.call(ctx, "sendChatAction", map[string]any{"chat_id": r.ChatID, "message_thread_id": r.ThreadID, "action": "typing"}, nil)
				}
			}
		}
	}
}
func (b *Bot) throttled() bool {
	b.mu.RLock()
	defer b.mu.RUnlock()
	return time.Now().Before(b.blockedUntil)
}

func (b *Bot) routeFor(ctx context.Context, sessionID string, chatID int64) route {
	b.mu.RLock()
	r, ok := b.routes[sessionID]
	b.mu.RUnlock()
	if ok {
		return r
	}
	if chatID == 0 {
		chatID = b.cfg.DefaultChatID
	}
	r.ChatID = chatID
	if chatID == 0 {
		return r
	}
	if b.cfg.ForumTopics && chatID < 0 {
		var topic struct {
			ThreadID int `json:"message_thread_id"`
		}
		if err := b.call(ctx, "createForumTopic", map[string]any{"chat_id": chatID, "name": truncate(sessionID, 100)}, &topic); err == nil {
			r.ThreadID = topic.ThreadID
		} else {
			_ = b.fail(err)
		}
	}
	b.mu.Lock()
	if existing, exists := b.routes[sessionID]; exists {
		r = existing
	} else {
		b.routes[sessionID] = r
		if b.selected[r] == "" {
			b.selected[r] = sessionID
		}
	}
	b.mu.Unlock()
	return r
}

func (b *Bot) button(label string, action callback) map[string]string {
	data := make([]byte, 16)
	if _, err := rand.Read(data); err != nil {
		return map[string]string{"text": label, "callback_data": "unavailable"}
	}
	token := hex.EncodeToString(data)
	b.mu.Lock()
	b.callbacks[token] = action
	b.mu.Unlock()
	return map[string]string{"text": label, "callback_data": token}
}

func (b *Bot) flushActions(ctx context.Context) {
	if b.throttled() {
		return
	}
	b.mu.Lock()
	actions := b.actions
	b.actions = make(map[string]notification)
	b.mu.Unlock()
	for key, action := range actions {
		if ctx.Err() != nil {
			return
		}
		var sessionID, text string
		if action.Approval != nil {
			sessionID = action.Approval.SessionID
		}
		if action.Question != nil {
			sessionID = action.Question.SessionID
		}
		if action.Artifact != nil {
			sessionID = action.Artifact.SessionID
		}
		r := b.routeFor(ctx, sessionID, action.ChatID)
		if r.ChatID == 0 {
			continue
		}
		var err error
		if action.Artifact != nil {
			err = b.upload(ctx, r, *action.Artifact)
		} else {
			keyboard := [][]map[string]string{}
			if app := action.Approval; app != nil {
				if app.Resolved || (!app.ExpiresAt.IsZero() && time.Now().After(app.ExpiresAt)) {
					continue
				}
				text = fmt.Sprintf("Approval required\nSession: %s\nTool: %s\n%s", app.SessionID, app.ToolName, app.Command)
				expires := app.ExpiresAt
				if expires.IsZero() {
					expires = time.Now().Add(2 * time.Minute)
				}
				yes, no := true, false
				keyboard = append(keyboard, []map[string]string{b.button("Allow", callback{ID: app.ID, Approved: &yes, ChatID: r.ChatID, Expires: expires}), b.button("Deny", callback{ID: app.ID, Approved: &no, ChatID: r.ChatID, Expires: expires})})
			}
			if q := action.Question; q != nil {
				text = q.QuestionText + "\n\n/answer " + q.QuestionID + " your answer"
				if q.IsMultiSelect {
					text += "\nSeparate multiple choices with |"
				} else {
					for _, option := range q.Options {
						keyboard = append(keyboard, []map[string]string{b.button(truncate(option, 60), callback{ID: q.QuestionID, Answers: []any{option}, ChatID: r.ChatID, Expires: time.Now().Add(10 * time.Minute)})})
					}
				}
			}
			err = b.call(ctx, "sendMessage", map[string]any{"chat_id": r.ChatID, "message_thread_id": r.ThreadID, "text": truncate(text, 4000), "reply_markup": map[string]any{"inline_keyboard": keyboard}}, nil)
		}
		if err != nil {
			_ = b.fail(err)
			action.Attempts++
			if action.Attempts < 4 || b.throttled() {
				b.mu.Lock()
				if _, exists := b.actions[key]; !exists {
					b.actions[key] = action
				}
				b.mu.Unlock()
			}
		}
	}
}

func (b *Bot) flushMessages(ctx context.Context) {
	if b.throttled() {
		return
	}
	b.mu.RLock()
	pending := make(map[string]pendingMessage, len(b.pending))
	for key, msg := range b.pending {
		pending[key] = msg
	}
	b.mu.RUnlock()
	for key, pending := range pending {
		if ctx.Err() != nil {
			return
		}
		r := b.routeFor(ctx, pending.Message.SessionID, pending.ChatID)
		if r.ChatID == 0 {
			b.mu.Lock()
			delete(b.pending, key)
			delete(b.generating, pending.Message.SessionID)
			b.mu.Unlock()
			continue
		}
		runes := []rune(pending.Message.Text)
		success := true
		for offset, part := 0, 0; offset < len(runes); offset, part = offset+3900, part+1 {
			end := offset + 3900
			if end > len(runes) {
				end = len(runes)
			}
			text := string(runes[offset:end])
			draftKey := fmt.Sprintf("%s:%d", key, part)
			b.mu.RLock()
			current := b.drafts[draftKey]
			b.mu.RUnlock()
			if current.MessageID != 0 && current.Text == text {
				continue
			}
			var err error
			if current.MessageID == 0 || current.Route != r {
				var sent telegramMessage
				err = b.call(ctx, "sendMessage", map[string]any{"chat_id": r.ChatID, "message_thread_id": r.ThreadID, "text": text}, &sent)
				if err == nil {
					current = draft{Route: r, MessageID: sent.ID, Text: text}
				}
			} else {
				err = b.call(ctx, "editMessageText", map[string]any{"chat_id": r.ChatID, "message_id": current.MessageID, "text": text}, nil)
				if err == nil {
					current.Text = text
				}
			}
			if err != nil {
				_ = b.fail(err)
				success = false
				break
			}
			b.mu.Lock()
			b.drafts[draftKey] = current
			b.mu.Unlock()
		}
		if success {
			b.mu.Lock()
			if latest, ok := b.pending[key]; ok && latest.Message.Rev == pending.Message.Rev && latest.Message.Text == pending.Message.Text {
				delete(b.pending, key)
			}
			if !pending.Message.Streaming {
				for draftKey := range b.drafts {
					if strings.HasPrefix(draftKey, key+":") {
						delete(b.drafts, draftKey)
					}
				}
			}
			b.mu.Unlock()
		}
	}
}

func (b *Bot) upload(ctx context.Context, r route, artifact protocol.ArtifactUpdatedEvent) error {
	var body bytes.Buffer
	form := multipart.NewWriter(&body)
	_ = form.WriteField("chat_id", strconv.FormatInt(r.ChatID, 10))
	if r.ThreadID != 0 {
		_ = form.WriteField("message_thread_id", strconv.Itoa(r.ThreadID))
	}
	file, err := form.CreateFormFile("document", filepath.Base(artifact.Path))
	if err != nil {
		return err
	}
	_, _ = io.WriteString(file, artifact.Content)
	_ = form.Close()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, b.cfg.APIBase+"/bot"+b.cfg.Token+"/sendDocument", &body)
	if err != nil {
		return fmt.Errorf("invalid Telegram upload endpoint")
	}
	req.Header.Set("Content-Type", form.FormDataContentType())
	response, err := b.http.Do(req)
	if err != nil {
		return fmt.Errorf("telegram document upload failed")
	}
	defer func() { _ = response.Body.Close() }()
	var result struct {
		OK bool `json:"ok"`
	}
	if json.NewDecoder(io.LimitReader(response.Body, 1024*1024)).Decode(&result) != nil || !result.OK {
		return fmt.Errorf("telegram document upload rejected")
	}
	return nil
}

// Package telegram is a pure-Go Telegram companion using the HTTP Bot API.
package telegram

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/approval"
	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type Handlers struct {
	Prompt   func(context.Context, string, string) error
	Approval func(context.Context, string, bool) error
	Answer   func(context.Context, string, []any) error
	Stop     func(context.Context, string) error
	Create   func(context.Context, string, string) (string, error)
}
type Config struct {
	Token          string
	AllowedUserIDs []int64
	DefaultChatID  int64
	ForumTopics    bool
	Handlers       Handlers
	APIBase        string // tests may provide an HTTP test server
	HTTPClient     *http.Client
}
type route struct {
	ChatID   int64
	ThreadID int
}
type callback struct {
	ID       string
	Approved *bool
	Answers  []any
	ChatID   int64
	Expires  time.Time
}
type draft struct {
	Route     route
	MessageID int
	Text      string
}
type pendingMessage struct {
	Message chat.Message
	ChatID  int64
}
type notification struct {
	ChatID   int64
	Approval *approval.PendingApproval
	Question *protocol.QuestionAskedEvent
	Artifact *protocol.ArtifactUpdatedEvent
	Attempts int
}

type Bot struct {
	mu                  sync.RWMutex
	cfg                 Config
	bus                 *events.Bus
	http                *http.Client
	running             bool
	lastError, username string
	cancel              context.CancelFunc
	wg                  sync.WaitGroup
	routes              map[string]route
	selected            map[route]string
	pending             map[string]pendingMessage
	drafts              map[string]draft
	actions             map[string]notification
	callbacks           map[string]callback
	generating          map[string]bool
	blockedUntil        time.Time
	wake                chan struct{}
}
type Status struct {
	Enabled   bool   `json:"enabled"`
	Running   bool   `json:"running"`
	Username  string `json:"username,omitempty"`
	LastError string `json:"lastError,omitempty"`
}
type telegramMessage struct {
	ID       int    `json:"message_id"`
	ThreadID int    `json:"message_thread_id"`
	Text     string `json:"text"`
	Chat     struct {
		ID int64 `json:"id"`
	} `json:"chat"`
	From *struct {
		ID int64 `json:"id"`
	} `json:"from"`
}
type update struct {
	ID       int64            `json:"update_id"`
	Message  *telegramMessage `json:"message"`
	Callback *struct {
		ID   string `json:"id"`
		Data string `json:"data"`
		From struct {
			ID int64 `json:"id"`
		} `json:"from"`
		Message *telegramMessage `json:"message"`
	} `json:"callback_query"`
}

func New(cfg Config, bus *events.Bus, _ *approval.Registry) *Bot {
	if cfg.APIBase == "" {
		cfg.APIBase = "https://api.telegram.org"
	}
	client := cfg.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: 35 * time.Second}
	}
	return &Bot{cfg: cfg, bus: bus, http: client, routes: make(map[string]route), selected: make(map[route]string), pending: make(map[string]pendingMessage), drafts: make(map[string]draft), actions: make(map[string]notification), callbacks: make(map[string]callback), generating: make(map[string]bool), wake: make(chan struct{}, 1)}
}

func (b *Bot) isUserAllowed(id int64) bool {
	for _, allowed := range b.cfg.AllowedUserIDs {
		if allowed == id {
			return true
		}
	}
	return false
}

func (b *Bot) Start(ctx context.Context) error {
	if b.cfg.Token == "" {
		return nil
	}
	if len(b.cfg.AllowedUserIDs) == 0 {
		return b.fail(fmt.Errorf("Telegram requires at least one allowed user ID"))
	}
	b.mu.Lock()
	if b.running {
		b.mu.Unlock()
		return nil
	}
	b.mu.Unlock()
	var me struct {
		Username string `json:"username"`
	}
	if err := b.call(ctx, "getMe", map[string]any{}, &me); err != nil {
		return b.fail(err)
	}
	ctx, cancel := context.WithCancel(ctx)
	b.mu.Lock()
	b.cancel, b.running, b.username = cancel, true, me.Username
	b.mu.Unlock()
	b.wg.Add(2)
	go func() { defer b.wg.Done(); b.poll(ctx) }()
	go func() { defer b.wg.Done(); b.outbox(ctx) }()
	return nil
}
func (b *Bot) Close() {
	b.mu.Lock()
	cancel := b.cancel
	b.running = false
	b.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	b.wg.Wait()
	b.http.CloseIdleConnections()
}
func (b *Bot) Status() Status {
	b.mu.RLock()
	defer b.mu.RUnlock()
	return Status{Enabled: b.cfg.Token != "", Running: b.running, Username: b.username, LastError: b.lastError}
}
func (b *Bot) fail(err error) error {
	if err != nil {
		b.mu.Lock()
		b.lastError = err.Error()
		b.mu.Unlock()
	}
	return err
}

func (b *Bot) call(ctx context.Context, method string, payload any, result any) error {
	data, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, b.cfg.APIBase+"/bot"+b.cfg.Token+"/"+method, bytes.NewReader(data))
	if err != nil {
		return fmt.Errorf("invalid Telegram endpoint")
	}
	req.Header.Set("Content-Type", "application/json")
	response, err := b.http.Do(req)
	if err != nil {
		return fmt.Errorf("Telegram %s request failed", method)
	} // never include the token-bearing URL
	defer func() { _ = response.Body.Close() }()
	var envelope struct {
		OK          bool            `json:"ok"`
		Result      json.RawMessage `json:"result"`
		Description string          `json:"description"`
		Parameters  struct {
			RetryAfter int `json:"retry_after"`
		} `json:"parameters"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, 8*1024*1024)).Decode(&envelope); err != nil {
		return fmt.Errorf("Telegram %s returned an invalid response", method)
	}
	if !envelope.OK {
		if envelope.Parameters.RetryAfter > 0 {
			b.mu.Lock()
			b.blockedUntil = time.Now().Add(time.Duration(envelope.Parameters.RetryAfter) * time.Second)
			b.mu.Unlock()
		}
		return fmt.Errorf("Telegram %s: %s", method, strings.ReplaceAll(envelope.Description, b.cfg.Token, "[redacted]"))
	}
	if result != nil {
		return json.Unmarshal(envelope.Result, result)
	}
	return nil
}

func (b *Bot) poll(ctx context.Context) {
	var offset int64
	for ctx.Err() == nil {
		var updates []update
		if err := b.call(ctx, "getUpdates", map[string]any{"offset": offset, "timeout": 25, "allowed_updates": []string{"message", "callback_query"}}, &updates); err != nil {
			if ctx.Err() != nil {
				return
			}
			_ = b.fail(err)
			select {
			case <-ctx.Done():
				return
			case <-time.After(2 * time.Second):
			}
			continue
		}
		for _, item := range updates {
			if item.ID >= offset {
				offset = item.ID + 1
				b.handleUpdate(ctx, item)
			}
		}
	}
}

func (b *Bot) handleUpdate(ctx context.Context, item update) {
	if item.Callback != nil {
		b.handleCallback(ctx, item)
		return
	}
	m := item.Message
	if m == nil || m.From == nil || !b.isUserAllowed(m.From.ID) {
		return
	}
	r := route{ChatID: m.Chat.ID, ThreadID: m.ThreadID}
	text := strings.TrimSpace(m.Text)
	parts := strings.SplitN(text, " ", 2)
	command := strings.SplitN(parts[0], "@", 2)[0]
	argument := ""
	if len(parts) == 2 {
		argument = strings.TrimSpace(parts[1])
	}
	reply := func(text string) {
		_ = b.call(ctx, "sendMessage", map[string]any{"chat_id": r.ChatID, "message_thread_id": r.ThreadID, "text": truncate(text, 4000)}, nil)
	}
	b.mu.RLock()
	sessionID := b.selected[r]
	b.mu.RUnlock()
	var err error
	switch command {
	case "/start", "/help":
		reply("OpenRemote\n/sessions — list sessions\n/use ID — select a session\n/new AGENT PATH — start a session\n/stop — stop selected session\n/answer ID TEXT — answer a question (separate multiple answers with |)\nSend any other text as a prompt to the selected session.")
	case "/health":
		reply("OpenRemote is running.")
	case "/sessions":
		if b.bus == nil {
			reply("No sessions available.")
			return
		}
		list, e := b.bus.ListSessions()
		if e != nil {
			err = e
			break
		}
		var lines []string
		for _, session := range list {
			lines = append(lines, fmt.Sprintf("%v (%v): %v", session["sessionId"], session["agentId"], session["status"]))
		}
		if len(lines) == 0 {
			lines = []string{"No sessions available."}
		}
		reply(strings.Join(lines, "\n"))
	case "/use":
		if b.bus == nil {
			reply("No sessions available.")
			return
		}
		list, e := b.bus.ListSessions()
		if e != nil {
			err = e
			break
		}
		found := false
		for _, session := range list {
			if session["sessionId"] == argument {
				found = true
				break
			}
		}
		if !found {
			reply("Session not found. Use /sessions.")
			return
		}
		b.mu.Lock()
		b.selected[r], b.routes[argument] = argument, r
		b.mu.Unlock()
		reply("Selected " + argument)
	case "/new":
		args := strings.SplitN(argument, " ", 2)
		if len(args) != 2 || b.cfg.Handlers.Create == nil {
			reply("Use /new AGENT /absolute/workspace/path")
			return
		}
		var id string
		id, err = b.cfg.Handlers.Create(ctx, args[0], strings.TrimSpace(args[1]))
		if err == nil {
			b.mu.Lock()
			b.selected[r], b.routes[id] = id, r
			b.mu.Unlock()
			reply("Started " + id)
		}
	case "/stop":
		if sessionID == "" || b.cfg.Handlers.Stop == nil {
			reply("Select a session with /use ID first.")
			return
		}
		err = b.cfg.Handlers.Stop(ctx, sessionID)
		if err == nil {
			reply("Session stopped.")
		}
	case "/answer":
		args := strings.SplitN(argument, " ", 2)
		if len(args) != 2 || b.cfg.Handlers.Answer == nil {
			reply("Use /answer QUESTION_ID your answer")
			return
		}
		var answers []any
		for _, answer := range strings.Split(args[1], "|") {
			answers = append(answers, strings.TrimSpace(answer))
		}
		err = b.cfg.Handlers.Answer(ctx, args[0], answers)
		if err == nil {
			reply("Answer delivered.")
		}
	default:
		if text == "" {
			return
		}
		if sessionID == "" || b.cfg.Handlers.Prompt == nil {
			reply("Select a session with /use ID first.")
			return
		}
		err = b.cfg.Handlers.Prompt(ctx, sessionID, text)
	}
	if err != nil {
		reply("Could not complete request: " + err.Error())
	}
}

func (b *Bot) handleCallback(ctx context.Context, item update) {
	q := item.Callback
	if q == nil || !b.isUserAllowed(q.From.ID) || q.Message == nil {
		return
	}
	b.mu.RLock()
	action, ok := b.callbacks[q.Data]
	b.mu.RUnlock()
	status := "This action expired."
	if ok && action.ChatID == q.Message.Chat.ID && time.Now().Before(action.Expires) {
		var err error
		if action.Approved != nil && b.cfg.Handlers.Approval != nil {
			err = b.cfg.Handlers.Approval(ctx, action.ID, *action.Approved)
		} else if action.Approved == nil && b.cfg.Handlers.Answer != nil {
			err = b.cfg.Handlers.Answer(ctx, action.ID, action.Answers)
		} else {
			err = fmt.Errorf("action handler unavailable")
		}
		if err == nil {
			status = "Delivered."
			b.mu.Lock()
			delete(b.callbacks, q.Data)
			b.mu.Unlock()
			_ = b.call(ctx, "editMessageReplyMarkup", map[string]any{"chat_id": q.Message.Chat.ID, "message_id": q.Message.ID, "reply_markup": map[string]any{"inline_keyboard": []any{}}}, nil)
		} else {
			status = truncate(err.Error(), 180)
		}
	}
	_ = b.call(ctx, "answerCallbackQuery", map[string]any{"callback_query_id": q.ID, "text": status}, nil)
}

func truncate(text string, max int) string {
	runes := []rune(text)
	if len(runes) <= max {
		return text
	}
	return string(runes[:max-1]) + "…"
}

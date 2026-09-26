package telegram

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/approval"
	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

func TestAutomaticRoutingPreservesSelectionAndDropsUnroutedDrafts(t *testing.T) {
	b := New(Config{Token: "test", DefaultChatID: 100, AllowedUserIDs: []int64{42}}, nil, nil)
	r := route{ChatID: 100}
	b.selected[r] = "explicit-session"
	if got := b.routeFor(context.Background(), "background-session", 0); got != r {
		t.Fatalf("route=%v", got)
	}
	if b.selected[r] != "explicit-session" {
		t.Fatal("automatic route replaced explicit selection")
	}
	unrouted := New(Config{Token: "test", AllowedUserIDs: []int64{42}}, nil, nil)
	unrouted.running = true
	unrouted.NotifyChatMessage(context.Background(), 0, chat.Message{SessionID: "s", ID: "m", Role: protocol.RoleAssistant, Text: "unrouted", Streaming: true, Rev: 1})
	unrouted.flushMessages(context.Background())
	if len(unrouted.pending) != 0 || len(unrouted.generating) != 0 {
		t.Fatal("unrouted draft retained indefinitely")
	}
}

func TestApprovalAuthorizationAndAgentDelivery(t *testing.T) {
	var mu sync.Mutex
	var sent []map[string]any
	api := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if strings.HasSuffix(r.URL.Path, "/sendMessage") {
			mu.Lock()
			sent = append(sent, body)
			mu.Unlock()
		}
		_, _ = fmt.Fprint(w, `{"ok":true,"result":{"message_id":7}}`)
	}))
	defer api.Close()
	deliveries := 0
	longID := "session-approval-" + strings.Repeat("long-native-id", 10)
	b := New(Config{Token: "test", APIBase: api.URL, AllowedUserIDs: []int64{42}, DefaultChatID: 100, Handlers: Handlers{Approval: func(_ context.Context, id string, approved bool) error {
		if id != longID || !approved {
			t.Errorf("wrong driver approval: %s %v", id, approved)
		}
		deliveries++
		return nil
	}}}, nil, nil)
	b.running = true
	b.NotifyApproval(context.Background(), 0, &approval.PendingApproval{ID: longID, SessionID: "s", Command: "go test ./...", ExpiresAt: time.Now().Add(time.Minute)})
	b.flushActions(context.Background())
	mu.Lock()
	button := sent[0]["reply_markup"].(map[string]any)["inline_keyboard"].([]any)[0].([]any)[0].(map[string]any)
	token := button["callback_data"].(string)
	mu.Unlock()
	if len(token) > 64 {
		t.Fatal("Telegram callback exceeds its byte limit")
	}
	var action update
	_ = json.Unmarshal([]byte(fmt.Sprintf(`{"callback_query":{"id":"callback","data":%q,"from":{"id":999},"message":{"message_id":7,"chat":{"id":100}}}}`, token)), &action)
	b.handleUpdate(context.Background(), action)
	if deliveries != 0 {
		t.Fatal("unauthorized callback reached agent")
	}
	action.Callback.From.ID = 42
	b.handleUpdate(context.Background(), action)
	if deliveries != 1 {
		t.Fatal("authorized approval never reached agent")
	}
	b.handleUpdate(context.Background(), action)
	if deliveries != 1 {
		t.Fatal("approval delivered twice")
	}
}

func TestDebouncedDraftsTopicsAndAttachments(t *testing.T) {
	var mu sync.Mutex
	counts := map[string]int{}
	var texts []string
	api := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		method := r.URL.Path[strings.LastIndex(r.URL.Path, "/")+1:]
		mu.Lock()
		defer mu.Unlock()
		counts[method]++
		if method == "sendDocument" {
			if err := r.ParseMultipartForm(1024 * 1024); err != nil {
				t.Error(err)
			}
			if r.FormValue("message_thread_id") != "12" {
				t.Error("attachment lost topic routing")
			}
		} else {
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			if method == "sendMessage" || method == "editMessageText" {
				texts = append(texts, body["text"].(string))
			}
		}
		if method == "createForumTopic" {
			_, _ = fmt.Fprint(w, `{"ok":true,"result":{"message_thread_id":12}}`)
		} else {
			_, _ = fmt.Fprint(w, `{"ok":true,"result":{"message_id":7}}`)
		}
	}))
	defer api.Close()
	b := New(Config{Token: "test", APIBase: api.URL, DefaultChatID: -100, ForumTopics: true}, nil, nil)
	b.running = true
	msg := chat.Message{ID: "m", SessionID: "s", Role: protocol.RoleAssistant, Text: "early", Rev: 1, Streaming: true}
	b.NotifyChatMessage(context.Background(), 0, msg)
	msg.Text, msg.Rev = "latest 🌍", 2
	b.NotifyChatMessage(context.Background(), 0, msg)
	b.flushMessages(context.Background())
	msg.Text, msg.Rev, msg.Streaming = "complete 🌍", 3, false
	b.NotifyChatMessage(context.Background(), 0, msg)
	b.flushMessages(context.Background())
	b.NotifyArtifact(context.Background(), 0, protocol.ArtifactUpdatedEvent{BaseEvent: protocol.BaseEvent{SessionID: "s"}, Path: "implementation_plan.md", Content: "# Plan"})
	b.flushActions(context.Background())
	mu.Lock()
	defer mu.Unlock()
	if counts["sendMessage"] != 1 || counts["editMessageText"] != 1 || counts["createForumTopic"] != 1 || counts["sendDocument"] != 1 {
		t.Fatalf("calls=%v", counts)
	}
	if len(texts) != 2 || texts[0] != "latest 🌍" || texts[1] != "complete 🌍" {
		t.Fatalf("texts=%v", texts)
	}
}

func TestBotRequiresAllowlistBeforeNetwork(t *testing.T) {
	b := New(Config{Token: "unused", APIBase: "http://127.0.0.1:1"}, nil, nil)
	if err := b.Start(context.Background()); err == nil || !strings.Contains(err.Error(), "allowed user") {
		t.Fatalf("start=%v", err)
	}
}

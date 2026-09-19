package codex

import (
	"context"
	"encoding/json"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/transport"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

type recordingSink struct {
	mu       sync.Mutex
	messages []chat.Message
	events   []any
}

func (*recordingSink) Bytes([]byte) {}
func (s *recordingSink) Message(msg chat.Message) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.messages = append(s.messages, msg)
}
func (s *recordingSink) Event(evt any) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.events = append(s.events, evt)
}
func (*recordingSink) Exit(int, string) {}

func TestAppServerMessagesAndApprovals(t *testing.T) {
	sink := &recordingSink{}
	s := &appSession{cfg: types.SessionConfig{SessionID: "s"}, sink: sink, items: make(map[string]chat.Message), approvals: make(map[string]approvalRequest)}
	feed := func(method, params string, id string) {
		s.handle(transport.Message{Method: method, Params: json.RawMessage(params), ID: json.RawMessage(id)})
	}
	feed("item/agentMessage/delta", `{"itemId":"i","delta":"Hello "}`, "")
	feed("item/agentMessage/delta", `{"itemId":"i","delta":"world"}`, "")
	feed("item/completed", `{"item":{"id":"i","type":"agentMessage","text":"Hello world"}}`, "")
	if len(sink.messages) != 3 || sink.messages[2].Streaming || sink.messages[2].Text != "Hello world" || sink.messages[2].Rev != 3 {
		t.Fatalf("messages: %+v", sink.messages)
	}
	feed("item/commandExecution/requestApproval", `{"command":"go test ./...","reason":"Run tests"}`, "42")
	event, ok := sink.events[0].(protocol.ApprovalRequestedEvent)
	if !ok || event.Command != "go test ./..." {
		t.Fatalf("approval: %+v", sink.events)
	}
	if string(s.approvals[event.ApprovalID].id) != "42" {
		t.Fatal("lost server request ID")
	}
	feed("item/completed", `{"item":{"id":"f","type":"fileChange","changes":[{"path":"main.go","diff":"+hello"}]}}`, "")
	if diff, ok := sink.events[1].(protocol.DiffGeneratedEvent); !ok || diff.FilePath != "main.go" {
		t.Fatalf("diff: %+v", sink.events)
	}
}

// Opt-in handshake uses no model inference and never sends a prompt.
func TestInstalledAppServerHandshake(t *testing.T) {
	if os.Getenv("OPENREMOTE_LIVE_CODEX") != "1" {
		t.Skip("set OPENREMOTE_LIVE_CODEX=1 for installed CLI handshake")
	}
	driver := NewDriver(pty.NewManager())
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Second)
	defer cancel()
	session, err := driver.Start(ctx, types.SessionConfig{SessionID: "handshake", CWD: t.TempDir()}, &recordingSink{})
	if err != nil {
		t.Fatal(err)
	}
	if err := session.Close(); err != nil {
		t.Fatal(err)
	}
}

package ptybase_test

import (
	"context"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/ptybase"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

type recordingSink struct {
	mu       sync.Mutex
	data     []byte
	messages []chat.Message
	events   []any
	exited   chan struct{}
	exitCode int
	signal   string
}

func newRecordingSink() *recordingSink {
	return &recordingSink{exited: make(chan struct{})}
}

func (s *recordingSink) Bytes(data []byte) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.data = append(s.data, data...)
}

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

func (s *recordingSink) Exit(code int, signal string) {
	s.mu.Lock()
	s.exitCode = code
	s.signal = signal
	s.mu.Unlock()
	select {
	case <-s.exited:
	default:
		close(s.exited)
	}
}

func (s *recordingSink) hasData() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.data) > 0
}

func (s *recordingSink) eventCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.events)
}

func TestStartPipeline(t *testing.T) {
	mgr := pty.NewManager()
	sink := newRecordingSink()

	var hookLines []string
	var hookMu sync.Mutex

	var command string
	var args []string
	if runtime.GOOS == "windows" {
		command, args = "cmd.exe", []string{"/c", "echo openremote-ptybase-hook"}
	} else {
		command, args = "sh", []string{"-c", "echo openremote-ptybase-hook"}
	}

	sess, err := ptybase.Start(context.Background(), types.SessionConfig{
		SessionID: "sess_ptybase_test",
		Cols:      120,
		Rows:      30,
	}, mgr, sink, ptybase.Opts{
		Command: command,
		Args:    args,
		Lexer:   chat.NewGenericLexer(),
		LineHook: func(sessionID, line string) []any {
			hookMu.Lock()
			hookLines = append(hookLines, line)
			hookMu.Unlock()
			if len(line) > 0 {
				return []any{protocol.AuthURLEvent{
					BaseEvent: protocol.BaseEvent{SessionID: sessionID},
					Type:      protocol.EventAuthURL,
					URL:       "https://example.com/login?x=1",
				}}
			}
			return nil
		},
	})
	if err != nil {
		t.Skipf("skipping: pty spawn unsupported: %v", err)
	}
	defer sess.Close()

	select {
	case <-sink.exited:
	case <-time.After(5 * time.Second):
		t.Fatal("timed out waiting for session exit")
	}

	if !sink.hasData() {
		t.Fatal("expected raw PTY output to reach sink")
	}

	hookMu.Lock()
	defer hookMu.Unlock()
	if len(hookLines) == 0 {
		t.Fatal("expected LineHook to observe committed screen lines")
	}

	if sink.eventCount() == 0 {
		t.Fatal("expected LineHook events to be forwarded to sink.Event")
	}
}

func TestPromptAndApproveOnDeadSession(t *testing.T) {
	sess := &ptybase.BaseSession{}
	if err := sess.Prompt("hello"); err == nil {
		t.Fatal("expected Prompt on non-running session to fail")
	}
	if err := sess.RawInput([]byte("x")); err == nil {
		t.Fatal("expected RawInput on non-running session to fail")
	}
	if err := sess.Approve("id", true); err == nil {
		t.Fatal("expected Approve on non-running session to fail")
	}
	if err := sess.Answer("id", "yes"); err == nil {
		t.Fatal("expected Answer on non-running session to fail")
	}
}

func TestStartWithMissingBinary(t *testing.T) {
	mgr := pty.NewManager()
	_, err := ptybase.Start(context.Background(), types.SessionConfig{
		SessionID: "sess_missing_bin",
	}, mgr, nil, ptybase.Opts{Command: "definitely-not-a-real-binary-xyz"})
	if err == nil {
		t.Fatal("expected Start with missing binary to fail")
	}
}

package events_test

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/events"
)

func TestSQLiteWALEventBus(t *testing.T) {
	tempDir := t.TempDir()

	bus, err := events.Open(tempDir)
	if err != nil {
		t.Fatalf("failed to open event bus: %v", err)
	}
	defer bus.Close()

	if err := bus.IntegrityCheck(); err != nil {
		t.Fatalf("sqlite integrity check failed: %v", err)
	}

	sessionID := "ses_test123"
	err = bus.UpsertSession(sessionID, "wks_abc", "claude-code", "/repo", "/repo", "/repo/worktree", "task/feat", "running")
	if err != nil {
		t.Fatalf("UpsertSession failed: %v", err)
	}

	// Append sequence of events
	seq1, err := bus.AppendEvent(sessionID, "stream.chunk", map[string]any{"chunk": "Hello World"})
	if err != nil {
		t.Fatalf("AppendEvent 1 failed: %v", err)
	}
	seq2, err := bus.AppendEvent(sessionID, "approval.requested", map[string]any{"toolName": "bash", "command": "ls"})
	if err != nil {
		t.Fatalf("AppendEvent 2 failed: %v", err)
	}

	if seq2 <= seq1 {
		t.Errorf("expected monotonic seq increase: %d <= %d", seq2, seq1)
	}

	// Catchup replay
	eventsReplayed, err := bus.GetEventsSince(sessionID, 0)
	if err != nil {
		t.Fatalf("GetEventsSince failed: %v", err)
	}
	if len(eventsReplayed) != 2 {
		t.Fatalf("expected 2 events, got %d", len(eventsReplayed))
	}

	// Replay from seq1
	fromSeq1, err := bus.GetEventsSince(sessionID, seq1)
	if err != nil {
		t.Fatalf("GetEventsSince seq1 failed: %v", err)
	}
	if len(fromSeq1) != 1 {
		t.Fatalf("expected 1 event, got %d", len(fromSeq1))
	}

	// List sessions
	sessions, err := bus.ListSessions()
	if err != nil {
		t.Fatalf("ListSessions failed: %v", err)
	}
	if len(sessions) != 1 || sessions[0]["sessionId"] != sessionID {
		t.Fatalf("unexpected sessions list: %+v", sessions)
	}
}

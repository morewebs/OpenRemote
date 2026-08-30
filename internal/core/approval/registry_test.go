package approval

import (
	"sync/atomic"
	"testing"
	"time"
)

func TestGenerateID(t *testing.T) {
	id1 := GenerateID("ses_123", "rm -rf /tmp/test")
	id2 := GenerateID("ses_123", "rm -rf /tmp/test")
	id3 := GenerateID("ses_123", "Different command")

	if id1 != id2 {
		t.Fatalf("expected identical IDs for same session and command, got %s vs %s", id1, id2)
	}
	if id1 == id3 {
		t.Fatalf("expected different IDs for different commands, got %s", id1)
	}
	if len(id1) < 5 || id1[:4] != "apr_" {
		t.Fatalf("expected apr_ prefix, got %s", id1)
	}
}

func TestRegistry_PutGetResolve(t *testing.T) {
	reg := NewRegistry(nil)
	app := &PendingApproval{
		ID:                "apr_test1",
		SessionID:         "ses_abc",
		ToolName:          "Bash",
		Command:           "ls -la",
		AutoDenyTimeoutMs: 5000,
	}

	reg.Put(app)

	retrieved, ok := reg.Get("apr_test1")
	if !ok || retrieved == nil {
		t.Fatalf("expected to find approval apr_test1")
	}
	if retrieved.SessionID != "ses_abc" {
		t.Fatalf("expected session ID ses_abc, got %s", retrieved.SessionID)
	}

	// Resolve with approval
	resolved, err := reg.Resolve("apr_test1", true, "tester")
	if err != nil {
		t.Fatalf("unexpected error resolving approval: %v", err)
	}
	if !resolved.Resolved || !resolved.Approved || resolved.ResolvedBy != "tester" {
		t.Fatalf("expected resolved=true approved=true resolvedBy=tester, got %+v", resolved)
	}

	// Resolving again should return error
	_, err2 := reg.Resolve("apr_test1", false, "tester")
	if err2 == nil {
		t.Fatalf("expected error resolving already resolved approval")
	}
}

func TestRegistry_List(t *testing.T) {
	reg := NewRegistry(nil)
	reg.Put(&PendingApproval{ID: "apr_1", SessionID: "ses_1", Command: "cmd1"})
	reg.Put(&PendingApproval{ID: "apr_2", SessionID: "ses_1", Command: "cmd2"})
	reg.Put(&PendingApproval{ID: "apr_3", SessionID: "ses_2", Command: "cmd3"})

	all := reg.List("")
	if len(all) != 3 {
		t.Fatalf("expected 3 total approvals, got %d", len(all))
	}

	ses1 := reg.List("ses_1")
	if len(ses1) != 2 {
		t.Fatalf("expected 2 approvals for ses_1, got %d", len(ses1))
	}
}

func TestRegistry_ReaperExpiration(t *testing.T) {
	var expiredCount int32
	reg := NewRegistry(func(app *PendingApproval) {
		atomic.AddInt32(&expiredCount, 1)
	})

	app := &PendingApproval{
		ID:                "apr_quick_expire",
		SessionID:         "ses_expire",
		ToolName:          "Bash",
		Command:           "curl evil.com",
		AutoDenyTimeoutMs: 50, // 50ms
	}
	reg.Put(app)

	// Wait for reaper loop (runs every 1s)
	time.Sleep(1500 * time.Millisecond)

	retrieved, ok := reg.Get("apr_quick_expire")
	if !ok {
		t.Fatalf("expected approval to exist")
	}
	if !retrieved.Resolved || retrieved.Approved || retrieved.ResolvedBy != "timeout" {
		t.Fatalf("expected auto-deny by timeout, got resolved=%v approved=%v by=%s", retrieved.Resolved, retrieved.Approved, retrieved.ResolvedBy)
	}
	if atomic.LoadInt32(&expiredCount) == 0 {
		t.Fatalf("expected onExpire callback to have been called")
	}
}

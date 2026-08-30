package telegram

import (
	"context"
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/approval"
	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

func TestBot_AllowedUsers(t *testing.T) {
	// 1. When AllowedUserIDs is empty, all users are allowed
	b1 := New(Config{Token: ""}, nil, nil)
	if !b1.isUserAllowed(12345) {
		t.Fatalf("expected all users allowed when list is empty")
	}

	// 2. When AllowedUserIDs is configured, only listed IDs allowed
	b2 := New(Config{
		Token:          "",
		AllowedUserIDs: []int64{100, 200, 300},
	}, nil, nil)
	if !b2.isUserAllowed(100) || !b2.isUserAllowed(200) {
		t.Fatalf("expected allowed user IDs to pass")
	}
	if b2.isUserAllowed(999) {
		t.Fatalf("expected unlisted user ID to be rejected")
	}
}

func TestBot_Status(t *testing.T) {
	b := New(Config{Token: ""}, nil, nil)
	st := b.Status()
	if st.Enabled || st.Running {
		t.Fatalf("expected disabled and not running when token is empty, got %+v", st)
	}

	b2 := New(Config{Token: "fake_token"}, nil, nil)
	st2 := b2.Status()
	if !st2.Enabled {
		t.Fatalf("expected enabled when token is provided")
	}
}

func TestBot_SafeNotificationsWhenNotRunning(t *testing.T) {
	b := New(Config{Token: ""}, nil, nil)
	ctx := context.Background()

	// Calling notifications on unstarted bot should not panic
	b.NotifyApproval(ctx, 12345, &approval.PendingApproval{
		ID:        "apr_test",
		SessionID: "ses_1",
		Command:   "ls",
	})

	b.NotifyChatMessage(ctx, 12345, chat.Message{
		ID:        "msg_1",
		SessionID: "ses_1",
		Role:      protocol.RoleAssistant,
		Text:      "Hello world",
		Streaming: false,
	})
}

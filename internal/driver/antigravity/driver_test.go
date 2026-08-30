package antigravity

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestAntigravityDriver_Metadata(t *testing.T) {
	ptyMgr := pty.NewManager()
	drv := NewDriver(ptyMgr)

	if drv.AgentID() != protocol.AgentAntigravity {
		t.Fatalf("expected agent id %s, got %s", protocol.AgentAntigravity, drv.AgentID())
	}
	if drv.DisplayName() != "Antigravity" {
		t.Fatalf("expected display name Antigravity, got %s", drv.DisplayName())
	}

	caps := drv.Capabilities()
	if !caps.SupportsTerminal || !caps.SupportsChatNative || !caps.SupportsApproval || !caps.SupportsDiff {
		t.Fatalf("expected all capability flags to be true, got %+v", caps)
	}
}

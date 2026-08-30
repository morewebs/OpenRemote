package opencode

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestOpenCodeDriver_Metadata(t *testing.T) {
	ptyMgr := pty.NewManager()
	drv := NewDriver(ptyMgr)

	if drv.AgentID() != protocol.AgentOpenCode {
		t.Fatalf("expected agent id %s, got %s", protocol.AgentOpenCode, drv.AgentID())
	}
	if drv.DisplayName() != "OpenCode" {
		t.Fatalf("expected display name OpenCode, got %s", drv.DisplayName())
	}

	caps := drv.Capabilities()
	if !caps.SupportsTerminal || !caps.SupportsChatNative || !caps.SupportsApproval || !caps.SupportsDiff {
		t.Fatalf("expected all capability flags to be true, got %+v", caps)
	}
}

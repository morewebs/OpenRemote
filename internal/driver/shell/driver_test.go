package shell

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestShellDriver_Metadata(t *testing.T) {
	ptyMgr := pty.NewManager()
	drv := NewDriver(ptyMgr)

	if drv.AgentID() != protocol.AgentShell {
		t.Fatalf("expected agent id %s, got %s", protocol.AgentShell, drv.AgentID())
	}
	if drv.DisplayName() != "System Shell" {
		t.Fatalf("expected display name System Shell, got %s", drv.DisplayName())
	}

	caps := drv.Capabilities()
	if !caps.SupportsTerminal {
		t.Fatalf("expected supportsTerminal to be true")
	}
	if caps.SupportsChatNative || caps.SupportsApproval || caps.SupportsDiff {
		t.Fatalf("expected raw shell capabilities to match spec")
	}

	if err := drv.Probe(); err != nil {
		t.Fatalf("expected shell probe to succeed: %v", err)
	}
}

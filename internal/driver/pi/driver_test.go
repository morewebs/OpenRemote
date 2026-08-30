package pi

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestPiDriver_Metadata(t *testing.T) {
	ptyMgr := pty.NewManager()
	drv := NewDriver(ptyMgr)

	if drv.AgentID() != protocol.AgentPi {
		t.Fatalf("expected agent id %s, got %s", protocol.AgentPi, drv.AgentID())
	}
	if drv.DisplayName() != "Pi (ACP)" {
		t.Fatalf("expected display name Pi (ACP), got %s", drv.DisplayName())
	}

	caps := drv.Capabilities()
	if !caps.SupportsTerminal || !caps.SupportsChatNative || !caps.SupportsApproval {
		t.Fatalf("expected capability flags to match ACP driver spec, got %+v", caps)
	}
	if caps.SupportsDiff {
		t.Fatalf("expected supportsDiff to be false for Pi driver")
	}
}

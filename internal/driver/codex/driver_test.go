package codex

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestCodexDriver_Metadata(t *testing.T) {
	ptyMgr := pty.NewManager()
	drv := NewDriver(ptyMgr)

	if drv.AgentID() != protocol.AgentCodex {
		t.Fatalf("expected agent id %s, got %s", protocol.AgentCodex, drv.AgentID())
	}
	if drv.DisplayName() != "OpenAI Codex" {
		t.Fatalf("expected display name OpenAI Codex, got %s", drv.DisplayName())
	}

	caps := drv.Capabilities()
	if !caps.SupportsTerminal || !caps.SupportsChatNative || !caps.SupportsApproval || !caps.SupportsDiff {
		t.Fatalf("expected all capability flags to be true, got %+v", caps)
	}
}

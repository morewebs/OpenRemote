package driver

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestDriverRegistry(t *testing.T) {
	ptyMgr := pty.NewManager()

	reg := NewRegistry(ptyMgr)

	expectedAgents := []protocol.AgentID{
		protocol.AgentClaude,
		protocol.AgentAntigravity,
		protocol.AgentOpenCode,
		protocol.AgentCodex,
		protocol.AgentPi,
		protocol.AgentShell,
	}

	for _, agentID := range expectedAgents {
		drv, ok := reg.Get(agentID)
		if !ok || drv == nil {
			t.Fatalf("expected driver for %s to be registered", agentID)
		}
		if drv.AgentID() != agentID {
			t.Fatalf("expected AgentID %s, got %s", agentID, drv.AgentID())
		}
		if drv.DisplayName() == "" {
			t.Fatalf("expected non-empty DisplayName for %s", agentID)
		}
	}

	list := reg.List()
	if len(list) != len(expectedAgents) {
		t.Fatalf("expected %d agents in List(), got %d", len(expectedAgents), len(list))
	}

	// Shell driver is always available on any host
	shellDrv, ok := reg.Get(protocol.AgentShell)
	if !ok {
		t.Fatalf("expected shell driver")
	}
	if err := shellDrv.Probe(); err != nil {
		t.Fatalf("expected shell probe to succeed: %v", err)
	}

	// Verify capability structure
	caps := shellDrv.Capabilities()
	if !caps.SupportsTerminal {
		t.Fatalf("expected shell to support terminal")
	}
}

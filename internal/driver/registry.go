package driver

import (
	"context"
	"fmt"

	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

// Registry holds all 5 agent drivers — spec 03.
type Registry struct {
	drivers map[protocol.AgentID]Driver
}

func NewRegistry(ptyManager *pty.Manager) *Registry {
	r := &Registry{drivers: make(map[protocol.AgentID]Driver)}
	// Stub drivers — real logic in subpackages (claude, antigravity, , codex, pi)
	for _, d := range []Driver{
		NewStubDriver(protocol.AgentClaude, "Claude Code", Capabilities{SupportsApprovals: true, SupportsDisambiguation: true, SupportsDiffStreams: true, SupportsWorktrees: true}),
		NewStubDriver(protocol.AgentAntigravity, "Antigravity", Capabilities{SupportsApprovals: true, SupportsDiffStreams: true, SupportsWorktrees: true}),
		NewStubDriver(protocol.AgentOpenCode, "OpenCode", Capabilities{SupportsApprovals: true, SupportsDisambiguation: true, SupportsDiffStreams: true, SupportsWorktrees: true}),
		NewStubDriver(protocol.AgentCodex, "Codex", Capabilities{SupportsDiffStreams: true, SupportsWorktrees: true}),
		NewStubDriver(protocol.AgentPi, "Pi", Capabilities{SupportsACP: true, SupportsApprovals: true}),
	} {
		r.drivers[d.AgentID()] = d
	}
	return r
}

func (r *Registry) Get(id protocol.AgentID) (Driver, bool) { d, ok := r.drivers[id]; return d, ok }

// StubDriver — placeholder until per-agent logic is implemented.
type StubDriver struct {
	agentID     protocol.AgentID
	displayName string
	caps        Capabilities
}

func NewStubDriver(id protocol.AgentID, name string, caps Capabilities) *StubDriver {
	return &StubDriver{agentID: id, displayName: name, caps: caps}
}
func (s *StubDriver) AgentID() protocol.AgentID { return s.agentID }
func (s *StubDriver) DisplayName() string       { return s.displayName }
func (s *StubDriver) Capabilities() Capabilities { return s.caps }
func (s *StubDriver) StartSession(_ context.Context, cfg SessionConfig) (Handle, error) {
	return Handle{}, fmt.Errorf("driver %s: not implemented", s.agentID)
}
func (s *StubDriver) StopSession(_ string) error                          { return nil }
func (s *StubDriver) SendPrompt(_, _ string) error                        { return fmt.Errorf("not implemented") }
func (s *StubDriver) SendRawInput(_ string, _ []byte) error               { return fmt.Errorf("not implemented") }
func (s *StubDriver) SendApproval(_, _ string, _ bool) error              { return fmt.Errorf("not implemented") }
func (s *StubDriver) SendAnswer(_, _ string, _ any) error                 { return fmt.Errorf("not implemented") }
func (s *StubDriver) ResizeViewport(_ string, _, _ int) error             { return nil }
func (s *StubDriver) StreamCh(_ string) <-chan []byte                     { ch := make(chan []byte); close(ch); return ch }
func (s *StubDriver) EventCh(_ string) <-chan Event                       { ch := make(chan Event); close(ch); return ch }
func (s *StubDriver) ExitCh(_ string) <-chan int                           { ch := make(chan int); close(ch); return ch }

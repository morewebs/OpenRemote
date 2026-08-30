package driver

import (
	"github.com/morewebs/OpenRemote/internal/driver/antigravity"
	"github.com/morewebs/OpenRemote/internal/driver/claude"
	"github.com/morewebs/OpenRemote/internal/driver/codex"
	"github.com/morewebs/OpenRemote/internal/driver/opencode"
	"github.com/morewebs/OpenRemote/internal/driver/pi"
	"github.com/morewebs/OpenRemote/internal/driver/shell"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

// Registry holds all registered agent drivers.
type Registry struct {
	drivers map[protocol.AgentID]types.Driver
	order   []protocol.AgentID
}

func NewRegistry(ptyManager *pty.Manager) *Registry {
	r := &Registry{
		drivers: make(map[protocol.AgentID]types.Driver),
		order: []protocol.AgentID{
			protocol.AgentClaude,
			protocol.AgentAntigravity,
			protocol.AgentOpenCode,
			protocol.AgentCodex,
			protocol.AgentPi,
			protocol.AgentShell,
		},
	}

	r.drivers[protocol.AgentClaude] = claude.NewDriver(ptyManager)
	r.drivers[protocol.AgentAntigravity] = antigravity.NewDriver(ptyManager)
	r.drivers[protocol.AgentOpenCode] = opencode.NewDriver(ptyManager)
	r.drivers[protocol.AgentCodex] = codex.NewDriver(ptyManager)
	r.drivers[protocol.AgentPi] = pi.NewDriver(ptyManager)
	r.drivers[protocol.AgentShell] = shell.NewDriver(ptyManager)

	return r
}

func (r *Registry) Get(id protocol.AgentID) (types.Driver, bool) {
	d, ok := r.drivers[id]
	return d, ok
}

func (r *Registry) List() []protocol.AgentInfo {
	var list []protocol.AgentInfo
	for _, id := range r.order {
		d, ok := r.drivers[id]
		if !ok {
			continue
		}
		info := protocol.AgentInfo{
			ID:           d.AgentID(),
			DisplayName:  d.DisplayName(),
			Capabilities: d.Capabilities(),
			Available:    true,
		}
		if err := d.Probe(); err != nil {
			info.Available = false
			info.Reason = err.Error()
		}
		list = append(list, info)
	}
	return list
}

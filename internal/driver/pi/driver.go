package pi

import (
	"context"
	"fmt"
	"os/exec"

	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

type Driver struct {
	ptyManager *pty.Manager
}

func NewDriver(ptyManager *pty.Manager) *Driver {
	return &Driver{ptyManager: ptyManager}
}

func (d *Driver) AgentID() protocol.AgentID {
	return protocol.AgentPi
}

func (d *Driver) DisplayName() string {
	return "Pi (RPC)"
}

func (d *Driver) Capabilities() protocol.DriverCapability {
	return protocol.DriverCapability{
		SupportsTerminal:   false,
		SupportsChatNative: true,
		SupportsApproval:   true,
		SupportsDiff:       false,
	}
}

func (d *Driver) Probe() error {
	for _, cand := range []string{"pi", "omp"} {
		if _, err := exec.LookPath(cand); err == nil {
			return nil
		}
	}
	return fmt.Errorf("pi / omp binary not found on PATH")
}

func (d *Driver) Start(ctx context.Context, cfg types.SessionConfig, sink types.Sink) (types.Session, error) {
	var bin string
	for _, cand := range []string{"pi", "omp"} {
		if path, err := exec.LookPath(cand); err == nil {
			bin = path
			break
		}
	}
	if bin == "" {
		return nil, fmt.Errorf("pi binary not found")
	}

	return startRPC(ctx, bin, cfg, sink)
}

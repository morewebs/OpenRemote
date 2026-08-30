package pi

import (
	"context"
	"fmt"
	"os/exec"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/ptybase"
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
	return "Pi (ACP)"
}

func (d *Driver) Capabilities() protocol.DriverCapability {
	return protocol.DriverCapability{
		SupportsTerminal:   true,
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
	return fmt.Errorf("pi / omp binary not found on PATH (ACP driver unverified on this host)")
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

	opts := ptybase.Opts{
		Command: bin,
		Args:    nil,
		Lexer:   chat.NewGenericLexer(),
		PromptFormatter: func(p string) []byte {
			return []byte(p + "\r\n")
		},
		ApproveKey: func(approved bool) []byte {
			if approved {
				return []byte("y\r\n")
			}
			return []byte("n\r\n")
		},
	}

	return ptybase.Start(ctx, cfg, d.ptyManager, sink, opts)
}

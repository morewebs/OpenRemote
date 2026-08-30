package shell

import (
	"context"
	"os"
	"runtime"

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
	return protocol.AgentShell
}

func (d *Driver) DisplayName() string {
	return "System Shell"
}

func (d *Driver) Capabilities() protocol.DriverCapability {
	return protocol.DriverCapability{
		SupportsTerminal:   true,
		SupportsChatNative: false,
		SupportsApproval:   false,
		SupportsDiff:       false,
	}
}

func (d *Driver) Probe() error {
	return nil
}

func (d *Driver) Start(ctx context.Context, cfg types.SessionConfig, sink types.Sink) (types.Session, error) {
	var cmd string
	var args []string

	if runtime.GOOS == "windows" {
		cmd = "powershell.exe"
		if comspec := os.Getenv("COMSPEC"); comspec != "" {
			cmd = comspec
		}
	} else {
		cmd = "/bin/bash"
		if sh := os.Getenv("SHELL"); sh != "" {
			cmd = sh
		}
	}

	opts := ptybase.Opts{
		Command: cmd,
		Args:    args,
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

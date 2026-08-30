package opencode

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
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
	return protocol.AgentOpenCode
}

func (d *Driver) DisplayName() string {
	return "OpenCode"
}

func (d *Driver) Capabilities() protocol.DriverCapability {
	return protocol.DriverCapability{
		SupportsTerminal:   true,
		SupportsChatNative: true,
		SupportsApproval:   true,
		SupportsDiff:       true,
	}
}

func (d *Driver) findBinary() (string, error) {
	candidates := []string{"opencode", "open-code", "opencode"}
	if runtime.GOOS == "windows" {
		candidates = append(candidates, "opencode.cmd", "opencode.exe", "open-code.cmd")
		if appdata := os.Getenv("APPDATA"); appdata != "" {
			candidates = append(candidates, filepath.Join(appdata, "npm", "opencode.cmd"))
			candidates = append(candidates, filepath.Join(appdata, "npm", "open-code.cmd"))
		}
	} else {
		if home, err := os.UserHomeDir(); err == nil {
			candidates = append(candidates, filepath.Join(home, ".npm-global", "bin", "opencode"))
			candidates = append(candidates, "/usr/local/bin/opencode")
		}
	}

	for _, cand := range candidates {
		if path, err := exec.LookPath(cand); err == nil {
			return path, nil
		}
		if fi, err := os.Stat(cand); err == nil && !fi.IsDir() {
			return cand, nil
		}
	}
	return "", fmt.Errorf("opencode binary not found in PATH or standard install locations")
}

func (d *Driver) Probe() error {
	_, err := d.findBinary()
	return err
}

func (d *Driver) Start(ctx context.Context, cfg types.SessionConfig, sink types.Sink) (types.Session, error) {
	bin, err := d.findBinary()
	if err != nil {
		return nil, err
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

package codex

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

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
	return protocol.AgentCodex
}

func (d *Driver) DisplayName() string {
	return "OpenAI Codex"
}

func (d *Driver) Capabilities() protocol.DriverCapability {
	return protocol.DriverCapability{
		SupportsTerminal:   false,
		SupportsChatNative: true,
		SupportsApproval:   true,
		SupportsDiff:       true,
	}
}

func (d *Driver) findBinary() (string, error) {
	names := []string{"codex"}
	var extras []string
	if runtime.GOOS == "windows" {
		names = append(names, "codex.cmd", "codex.exe")
		if appdata := os.Getenv("APPDATA"); appdata != "" {
			extras = append(extras, filepath.Join(appdata, "npm", "codex.cmd"))
		}
	} else {
		if home, err := os.UserHomeDir(); err == nil {
			extras = append(extras, filepath.Join(home, ".npm-global", "bin", "codex"))
			extras = append(extras, "/usr/local/bin/codex")
		}
	}
	p, err := ptybase.FindBinary(names, extras)
	if err != nil {
		return "", fmt.Errorf("codex binary not found in PATH or standard install locations")
	}
	return p, nil
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

	return startAppServer(ctx, bin, cfg, sink)
}

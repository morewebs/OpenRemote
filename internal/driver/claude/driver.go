package claude

import (
	"context"
	"fmt"
	"os"
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
	return protocol.AgentClaude
}

func (d *Driver) DisplayName() string {
	return "Claude Code"
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
	names := []string{"claude"}
	var extras []string
	if runtime.GOOS == "windows" {
		names = append(names, "claude.cmd", "claude.exe")
		if appdata := os.Getenv("APPDATA"); appdata != "" {
			extras = append(extras, filepath.Join(appdata, "npm", "claude.cmd"))
		}
		if localappdata := os.Getenv("LOCALAPPDATA"); localappdata != "" {
			extras = append(extras, filepath.Join(localappdata, "Programs", "Claude", "claude.exe"))
		}
	} else {
		if home, err := os.UserHomeDir(); err == nil {
			extras = append(extras, filepath.Join(home, ".npm-global", "bin", "claude"))
			extras = append(extras, "/usr/local/bin/claude")
		}
	}
	p, err := ptybase.FindBinary(names, extras)
	if err != nil {
		return "", fmt.Errorf("claude binary not found in PATH or standard install locations")
	}
	return p, nil
}

// buildArgs assembles the CLI launch arguments. Terminal mode passes
// Remote-control
// mode additionally attaches the session to a titled Claude.ai session.
func buildArgs(cfg types.SessionConfig) []string {
	var args []string
	if cfg.RemoteControl {
		title := cfg.TaskName
		if title == "" {
			title = cfg.SessionID
		}
		args = append(args, "--remote-control", title)
	}
	return args
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
		Args:    buildArgs(cfg),
		Lexer:   chat.NewClaudeLexer(),
		PromptFormatter: func(p string) []byte {
			return BracketedPaste(p)
		},
		// Claude's permission dialog is numbered: 1=Yes, 2=Yes and don't ask
		// again, 3=No (esc). Denying must send 3, not 2.
		ApproveKey: func(approved bool) []byte {
			if approved {
				return []byte("1\r")
			}
			return []byte("3\r")
		},
		LineHook: LineHook,
	}
	env := make(map[string]string, len(cfg.Env)+1)
	for key, value := range cfg.Env {
		env[key] = value
	}
	env["DISABLE_AUTOUPDATER"] = "1"
	cfg.Env = env

	return ptybase.Start(ctx, cfg, d.ptyManager, sink, opts)
}

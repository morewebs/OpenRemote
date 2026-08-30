package claude

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
	candidates := []string{"claude"}
	if runtime.GOOS == "windows" {
		candidates = append(candidates, "claude.cmd", "claude.exe")
		if appdata := os.Getenv("APPDATA"); appdata != "" {
			candidates = append(candidates, filepath.Join(appdata, "npm", "claude.cmd"))
		}
		if localappdata := os.Getenv("LOCALAPPDATA"); localappdata != "" {
			candidates = append(candidates, filepath.Join(localappdata, "Programs", "Claude", "claude.exe"))
		}
	} else {
		if home, err := os.UserHomeDir(); err == nil {
			candidates = append(candidates, filepath.Join(home, ".npm-global", "bin", "claude"))
			candidates = append(candidates, "/usr/local/bin/claude")
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
	return "", fmt.Errorf("claude binary not found in PATH or standard install locations")
}

// buildArgs assembles the CLI launch arguments. Terminal mode passes
// --no-auto-updater to prevent background CLI updates mid-turn; remote-control
// mode additionally attaches the session to a titled Claude.ai session.
func buildArgs(cfg types.SessionConfig) []string {
	args := []string{"--no-auto-updater"}
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
		LineHook: DetectLoginURL,
	}

	return ptybase.Start(ctx, cfg, d.ptyManager, sink, opts)
}

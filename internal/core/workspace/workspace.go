package workspace

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func NewID() string {
	b := make([]byte, 4)
	_, _ = rand.Read(b)
	return "wks_" + hex.EncodeToString(b)
}

func NewSessionID() string {
	b := make([]byte, 4)
	_, _ = rand.Read(b)
	return "ses_" + hex.EncodeToString(b)
}

// EnsureWorktree provisions `git worktree add` for isolated tasks.
// Returns (worktreePath, branchName) or ("", "", nil) when useWorktree==false.
func EnsureWorktree(cwd, taskName string, useWorktree bool) (string, string, error) {
	if !useWorktree {
		return "", "", nil
	}
	safe := sanitizeBranch(taskName)
	if safe == "" {
		b := make([]byte, 3)
		_, _ = rand.Read(b)
		safe = "task-" + hex.EncodeToString(b)
	}
	branch := "task/" + safe
	worktreeDir := filepath.Join(cwd, ".openremote", "worktrees", safe)
	if _, err := os.Stat(worktreeDir); err == nil {
		return worktreeDir, branch, nil
	}
	if err := os.MkdirAll(filepath.Dir(worktreeDir), 0o755); err != nil {
		return "", "", err
	}
	cmd := exec.Command("git", "worktree", "add", worktreeDir, "-b", branch)
	cmd.Dir = cwd
	out, err := cmd.CombinedOutput()
	if err != nil {
		// maybe branch exists — try without -b
		cmd2 := exec.Command("git", "worktree", "add", worktreeDir, branch)
		cmd2.Dir = cwd
		if out2, err2 := cmd2.CombinedOutput(); err2 != nil {
			return "", "", fmt.Errorf("git worktree add: %v: %s / %s", err, string(out), string(out2))
		}
	}
	return worktreeDir, branch, nil
}

func RemoveWorktree(cwd, worktreePath string) error {
	cmd := exec.Command("git", "worktree", "remove", "--force", worktreePath)
	cmd.Dir = cwd
	out, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("git worktree remove: %w: %s", err, string(out))
	}
	return nil
}

func IsSafePath(cwd, target string) bool {
	absCwd, err1 := filepath.Abs(cwd)
	absTarget, err2 := filepath.Abs(target)
	if err1 != nil || err2 != nil {
		return false
	}
	rel, err := filepath.Rel(absCwd, absTarget)
	if err != nil {
		return false
	}
	return !strings.HasPrefix(rel, ".."+string(filepath.Separator)) && rel != ".."
}

func sanitizeBranch(s string) string {
	s = strings.TrimSpace(strings.ToLower(s))
	if s == "" {
		return ""
	}
	var b strings.Builder
	for _, r := range s {
		if (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9') || r == '-' || r == '_' {
			b.WriteRune(r)
		} else if r == ' ' || r == '/' {
			b.WriteRune('-')
		}
	}
	out := strings.Trim(b.String(), "-")
	if len(out) > 40 {
		out = out[:40]
	}
	return out
}

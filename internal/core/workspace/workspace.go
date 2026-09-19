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
//
// Notes on repo state:
//   - A dirty main worktree (uncommitted changes) does NOT block creation: the
//     new worktree is checked out from HEAD, so uncommitted changes stay in the
//     source worktree and are deliberately not carried over.
//   - Stale bookkeeping (directory deleted by hand, branch already checked out
//     elsewhere, leftover admin entries) is repaired with `git worktree prune`
//     before the create attempt.
//   - If the directory already exists and looks like a live worktree (has the
//     `.git` pointer file), it is reused as-is — idempotent re-attach.
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

	// Reuse only if it is actually a worktree (dir present AND contains the
	// `.git` pointer file git writes for secondary worktrees). A bare leftover
	// directory without `.git` would make `git worktree add` fail with
	// "already exists", so fall through to prune+retry in that case.
	if fi, err := os.Stat(worktreeDir); err == nil && fi.IsDir() {
		if _, gerr := os.Stat(filepath.Join(worktreeDir, ".git")); gerr == nil {
			return worktreeDir, branch, nil
		}
	}

	if err := os.MkdirAll(filepath.Dir(worktreeDir), 0o755); err != nil {
		return "", "", err
	}

	// Best-effort repair of stale registrations (never fatal): handles the
	// "missing but still registered" case and frees the branch if its old
	// worktree directory was removed out from under git.
	prune := exec.Command("git", "worktree", "prune")
	prune.Dir = cwd
	_, _ = prune.CombinedOutput()

	out, err := runWorktreeAdd(cwd, worktreeDir, branch, true)
	if err == nil {
		return worktreeDir, branch, nil
	}
	// Branch may already exist from a previous task — retry without -b.
	out2, err2 := runWorktreeAdd(cwd, worktreeDir, branch, false)
	if err2 == nil {
		return worktreeDir, branch, nil
	}
	return "", "", fmt.Errorf("git worktree add: create %q (branch %q) failed: %v: %s; retry without -b failed: %v: %s",
		worktreeDir, branch, err, firstLine(out), err2, firstLine(out2))
}

// runWorktreeAdd executes `git worktree add <dir> [-b] <branch>` in cwd.
// A leftover empty directory (created by MkdirAll or a prior failed attempt)
// makes git refuse ("already exists"), so remove an empty dir and try once more.
func runWorktreeAdd(cwd, dir, branch string, createBranch bool) ([]byte, error) {
	args := []string{"worktree", "add"}
	if createBranch {
		args = append(args, "-b", branch)
	} else {
		args = append(args, branch)
	}
	args = append(args, dir)
	cmd := exec.Command("git", args...)
	cmd.Dir = cwd
	out, err := cmd.CombinedOutput()
	if err != nil && dirExistsButEmpty(dir) {
		_ = os.Remove(dir)
		out, err = cmd2(cwd, args).CombinedOutput()
	}
	return out, err
}

func cmd2(cwd string, args []string) *exec.Cmd {
	c := exec.Command("git", args...)
	c.Dir = cwd
	return c
}

func dirExistsButEmpty(dir string) bool {
	entries, err := os.ReadDir(dir)
	return err == nil && len(entries) == 0
}

func firstLine(b []byte) string {
	s := strings.TrimSpace(string(b))
	if i := strings.IndexByte(s, '\n'); i >= 0 {
		return s[:i]
	}
	return s
}

// RemoveWorktree removes a registered worktree. When the directory is already
// gone (deleted out-of-band), it falls back to `git worktree prune` so the
// branch is released and the call still succeeds — callers treat removal as
// idempotent cleanup, not a hard assertion.
func RemoveWorktree(cwd, worktreePath string) error {
	cmd := exec.Command("git", "worktree", "remove", worktreePath)
	cmd.Dir = cwd
	out, err := cmd.CombinedOutput()
	if err != nil {
		if dirMissingOrEmpty(worktreePath) {
			// Directory already deleted — just drop the stale registration.
			prune := exec.Command("git", "worktree", "prune")
			prune.Dir = cwd
			if _, perr := prune.CombinedOutput(); perr == nil {
				return nil
			}
		}
		return fmt.Errorf("git worktree remove: %w: %s", err, string(out))
	}
	return nil
}

func dirMissingOrEmpty(dir string) bool {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return os.IsNotExist(err)
	}
	return len(entries) == 0
}

// IsSafePath verifies that target is contained within base directory without
// escaping via "..", absolute-path confusion, or symlinks.
//
// Symlink handling: for paths that do not exist yet (a common case — the API
// validates the destination *before* creating it), only lexical checks would
// trust a parent directory that is itself a symlink pointing outside base.
// resolveLoose therefore walks up to the deepest existing ancestor, fully
// resolves it with EvalSymlinks, and re-appends the non-existent tail. This
// closes the "symlinked ancestor" escape (base/link/evil where link -> /etc).
//
// Residual TOCTOU: between this check and the actual open the filesystem may
// change (an ancestor could be swapped for a symlink afterwards). Closing that
// fully requires opening with O_NOFOLLOW / resolving at syscall time (e.g.
// openat2) and is the caller's responsibility; this function is a validation
// gate, not an enforced sandbox.
func IsSafePath(base, target string) bool {
	absBase, err1 := filepath.Abs(base)
	absTarget, err2 := filepath.Abs(target)
	if err1 != nil || err2 != nil {
		return false
	}
	absBase = resolveLoose(absBase)
	absTarget = resolveLoose(absTarget)

	rel, err := filepath.Rel(absBase, absTarget)
	if err != nil {
		// Different volumes on Windows — not contained.
		return false
	}
	if filepath.IsAbs(rel) {
		return false
	}
	// Normalize to '/' so the prefix test is correct on every platform:
	// filepath.Rel can surface mixed separators when inputs use slashes.
	rel = filepath.ToSlash(rel)
	return rel == "." || (rel != ".." && !strings.HasPrefix(rel, "../"))
}

// resolveLoose returns the fully symlink-resolved form of p when p exists, or
// the deepest existing ancestor resolved with the remaining (not-yet-existing)
// suffix re-joined when it does not. Falls back to p unchanged if no ancestor
// can be resolved.
func resolveLoose(p string) string {
	if resolved, err := filepath.EvalSymlinks(p); err == nil {
		return resolved
	}
	var tail []string
	cur := p
	for {
		parent := filepath.Dir(cur)
		if parent == cur { // reached root (volume root on Windows, "/" on unix)
			return p
		}
		tail = append(tail, filepath.Base(cur))
		cur = parent
		if resolved, err := filepath.EvalSymlinks(cur); err == nil {
			for i := len(tail) - 1; i >= 0; i-- {
				resolved = filepath.Join(resolved, tail[i])
			}
			return resolved
		}
	}
}

// IsSafePathAny checks if target is safely within any allowed root directory.
func IsSafePathAny(roots []string, target string) bool {
	for _, root := range roots {
		if root != "" && IsSafePath(root, target) {
			return true
		}
	}
	return false
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

package workspace_test

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/workspace"
)

func TestDirtyWorktreeIsRetained(t *testing.T) {
	root := t.TempDir()
	git := func(args ...string) {
		t.Helper()
		cmd := exec.Command("git", args...)
		cmd.Dir = root
		if output, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v: %s", args, err, output)
		}
	}
	git("init")
	git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "initial")
	path, _, err := workspace.EnsureWorktree(root, "preserve", true)
	if err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(path, "important.txt")
	if err := os.WriteFile(file, []byte("keep this work"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := workspace.RemoveWorktree(root, path); err == nil {
		t.Fatal("dirty worktree removal succeeded")
	}
	if content, err := os.ReadFile(file); err != nil || string(content) != "keep this work" {
		t.Fatalf("work lost: %q, %v", content, err)
	}
	if err := os.Remove(file); err != nil {
		t.Fatal(err)
	}
	if err := workspace.RemoveWorktree(root, path); err != nil {
		t.Fatal(err)
	}
}

func TestPathSafety(t *testing.T) {
	cwd, err := filepath.Abs(".")
	if err != nil {
		t.Fatal(err)
	}

	safeChild := filepath.Join(cwd, "some", "nested", "file.txt")
	if !workspace.IsSafePath(cwd, safeChild) {
		t.Errorf("expected %s to be safe under %s", safeChild, cwd)
	}

	unsafeEscape := filepath.Join(cwd, "..", "..", "windows", "system32")
	if workspace.IsSafePath(cwd, unsafeEscape) {
		t.Errorf("expected %s to be unsafe under %s", unsafeEscape, cwd)
	}
}

func TestIDGeneration(t *testing.T) {
	wksID := workspace.NewID()
	if len(wksID) != 12 || wksID[:4] != "wks_" {
		t.Errorf("invalid workspace id format: %s", wksID)
	}

	sesID := workspace.NewSessionID()
	if len(sesID) != 12 || sesID[:4] != "ses_" {
		t.Errorf("invalid session id format: %s", sesID)
	}
}

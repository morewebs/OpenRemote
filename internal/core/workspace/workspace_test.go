package workspace_test

import (
	"path/filepath"
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/workspace"
)

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

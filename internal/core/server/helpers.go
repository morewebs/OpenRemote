package server

import (
	"encoding/json"
	"os"
	"os/exec"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

func deref(s *string) string {
	if s == nil {
		return ""
	}
	return *s
}

func strPtr(s string) *string {
	if s == "" {
		return nil
	}
	return &s
}

func jsonEscape(s string) string {
	b, _ := json.Marshal(s)
	return string(b)
}

func listFiles(dir string) ([]protocol.FileEntry, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}
	var out []protocol.FileEntry
	for _, e := range entries {
		if e.Name() == ".git" || e.Name() == ".openremote" {
			continue
		}
		info, _ := e.Info()
		var size int64
		if info != nil {
			size = info.Size()
		}
		out = append(out, protocol.FileEntry{Name: e.Name(), IsDir: e.IsDir(), Size: size})
	}
	return out, nil
}

func gitDiff(cwd string) string {
	cmd := exec.Command("git", "diff", "--no-color")
	cmd.Dir = cwd
	out, err := cmd.CombinedOutput()
	if err != nil {
		return ""
	}
	return string(out)
}

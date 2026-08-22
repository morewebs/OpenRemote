package server

import (
	"os"
	"os/exec"
	"path/filepath"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

func listFilesImpl(dir string) ([]protocol.FileEntry, error) {
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
	_ = filepath.Separator
	return out, nil
}

func gitDiffImpl(cwd string) string {
	cmd := exec.Command("git", "diff", "--no-color")
	cmd.Dir = cwd
	out, err := cmd.CombinedOutput()
	if err != nil {
		return ""
	}
	return string(out)
}

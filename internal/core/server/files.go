package server

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

// Root.Open enforces containment at open time, including concurrent symlink
// changes; the earlier lexical check alone cannot provide that guarantee.
func openWorkspaceFile(roots []string, path string) (*os.File, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	for _, base := range roots {
		base, err = filepath.Abs(base)
		if err != nil {
			continue
		}
		rel, err := filepath.Rel(base, absolute)
		if err != nil || filepath.IsAbs(rel) || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			continue
		}
		root, err := os.OpenRoot(base)
		if err != nil {
			continue
		}
		file, err := root.Open(rel)
		_ = root.Close()
		if err == nil {
			return file, nil
		}
	}
	return nil, fmt.Errorf("path unavailable or outside allowed roots")
}

func listWorkspaceFiles(roots []string, path string) ([]protocol.FileEntry, error) {
	file, err := openWorkspaceFile(roots, path)
	if err != nil {
		return nil, err
	}
	defer func() { _ = file.Close() }()
	entries, err := file.ReadDir(-1)
	if err != nil {
		return nil, err
	}
	list := make([]protocol.FileEntry, 0, len(entries))
	for _, entry := range entries {
		if entry.Name() == ".git" || entry.Name() == ".openremote" {
			continue
		}
		info, err := entry.Info()
		if err != nil {
			continue
		}
		list = append(list, protocol.FileEntry{Name: entry.Name(), Path: filepath.Join(path, entry.Name()), IsDir: entry.IsDir(), Size: info.Size()})
	}
	sort.Slice(list, func(i, j int) bool {
		if list[i].IsDir != list[j].IsDir {
			return list[i].IsDir
		}
		return strings.ToLower(list[i].Name) < strings.ToLower(list[j].Name)
	})
	return list, nil
}

func (s *Server) handleFile(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		methodNotAllowed(w, http.MethodGet)
		return
	}
	path := r.URL.Query().Get("path")
	if path == "" {
		http.Error(w, "path required", http.StatusBadRequest)
		return
	}
	file, err := openWorkspaceFile(s.cfg.AllowedRoots, path)
	if err != nil {
		http.Error(w, err.Error(), http.StatusForbidden)
		return
	}
	defer func() { _ = file.Close() }()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() {
		http.Error(w, "not a regular file", http.StatusBadRequest)
		return
	}
	const limit = 256 * 1024
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	truncated := len(data) > limit
	if truncated {
		data = data[:limit]
		for len(data) > 0 && !utf8.Valid(data) && limit-len(data) < 4 {
			data = data[:len(data)-1]
		}
	}
	if !utf8.Valid(data) || strings.ContainsRune(string(data), '\x00') {
		http.Error(w, "This binary file cannot be previewed as text.", http.StatusUnsupportedMediaType)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{"path": path, "content": string(data), "size": info.Size(), "truncated": truncated})
}

func workspaceDiff(ctx context.Context, cwd string) (string, error) {
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, "git", "diff", "--no-color", "--no-ext-diff", "--no-textconv", "HEAD", "--")
	cmd.Dir = cwd
	output, err := cmd.StdoutPipe()
	if err != nil {
		return "", err
	}
	if err := cmd.Start(); err != nil {
		return "", err
	}
	data, err := io.ReadAll(io.LimitReader(output, 2*1024*1024+1))
	if len(data) > 2*1024*1024 {
		_ = cmd.Process.Kill()
		_ = cmd.Wait()
		return "", fmt.Errorf("diff exceeds 2 MB; review it in the local workspace")
	}
	waitErr := cmd.Wait()
	if err != nil {
		return "", err
	}
	if waitErr != nil {
		return "", fmt.Errorf("Git diff unavailable; the workspace must contain an initial commit")
	}
	return string(data), nil
}

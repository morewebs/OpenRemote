package antigravity

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/fsnotify/fsnotify"
	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/core/workspace"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type fileCursor struct {
	offset  int64
	pending []byte
	info    os.FileInfo
	hash    [32]byte
	seen    bool
}
type artifactWatcher struct {
	root, sessionID string
	sink            types.Sink
	files           map[string]*fileCursor
	watcher         *fsnotify.Watcher
	cancel          context.CancelFunc
	done            chan struct{}
	once            sync.Once
	subagentIndex   int
}

func startWatcher(ctx context.Context, cfg types.SessionConfig, sink types.Sink) (*artifactWatcher, error) {
	root := cfg.CWD
	if cfg.WorktreePath != "" {
		root = cfg.WorktreePath
	}
	watcher, err := fsnotify.NewWatcher()
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(ctx)
	w := &artifactWatcher{root: root, sessionID: cfg.SessionID, sink: sink, files: make(map[string]*fileCursor), watcher: watcher, cancel: cancel, done: make(chan struct{})}
	for _, dir := range []string{"", ".antigravity", ".gemini/antigravity"} {
		path := filepath.Join(root, filepath.FromSlash(dir))
		_ = watcher.Add(path)
		for _, name := range []string{"transcript.jsonl", "implementation_plan.md", "walkthrough.md"} {
			file := filepath.Join(path, name)
			cursor := &fileCursor{}
			if info, err := os.Stat(file); err == nil && workspace.IsSafePath(root, file) {
				cursor.info = info
				if name == "transcript.jsonl" {
					cursor.offset = info.Size()
				} else if data, err := readArtifact(file); err == nil {
					cursor.hash = sha256.Sum256(data)
					cursor.seen = true
				}
			}
			w.files[file] = cursor
		}
	}
	go func() {
		defer close(w.done)
		defer watcher.Close()
		defer w.poll()
		ticker := time.NewTicker(300 * time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case _, ok := <-watcher.Events:
				if !ok {
					return
				}
				w.poll()
			case _, ok := <-watcher.Errors:
				if !ok {
					return
				}
			case <-ticker.C:
				w.poll()
			}
		}
	}()
	return w, nil
}

func readArtifact(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, 4*1024*1024+1))
	if len(data) > 4*1024*1024 {
		return nil, fmt.Errorf("artifact exceeds 4 MB")
	}
	return data, err
}

func (w *artifactWatcher) poll() {
	for path, cursor := range w.files {
		if !workspace.IsSafePath(w.root, path) {
			continue
		}
		info, err := os.Stat(path)
		if err != nil || !info.Mode().IsRegular() {
			continue
		}
		if filepath.Base(path) == "transcript.jsonl" {
			if info.Size() < cursor.offset || (cursor.info != nil && !os.SameFile(cursor.info, info)) {
				cursor.offset = 0
				cursor.pending = nil
			}
			cursor.info = info
			if info.Size() == cursor.offset {
				continue
			}
			file, err := os.Open(path)
			if err != nil {
				continue
			}
			_, err = file.Seek(cursor.offset, io.SeekStart)
			if err != nil {
				_ = file.Close()
				continue
			}
			data, err := io.ReadAll(io.LimitReader(file, 4*1024*1024))
			_ = file.Close()
			if err != nil {
				continue
			}
			cursor.offset += int64(len(data))
			cursor.pending = append(cursor.pending, data...)
			for {
				end := bytes.IndexByte(cursor.pending, '\n')
				if end < 0 {
					break
				}
				w.transcriptLine(cursor.pending[:end])
				cursor.pending = cursor.pending[end+1:]
			}
			if len(cursor.pending) > 4*1024*1024 {
				cursor.pending = nil
			}
			continue
		}
		data, err := readArtifact(path)
		if err != nil {
			continue
		}
		hash := sha256.Sum256(data)
		if cursor.seen && hash == cursor.hash {
			continue
		}
		cursor.hash, cursor.seen = hash, true
		rel, _ := filepath.Rel(w.root, path)
		kind := "plan"
		if filepath.Base(path) == "walkthrough.md" {
			kind = "walkthrough"
		}
		w.sink.Event(protocol.ArtifactUpdatedEvent{BaseEvent: protocol.BaseEvent{SessionID: w.sessionID, Timestamp: protocol.NowMillis()}, Type: protocol.EventArtifactUpdated, Path: filepath.ToSlash(rel), Kind: kind, Content: string(data)})
	}
}

func (w *artifactWatcher) transcriptLine(line []byte) {
	var event struct {
		Type, Name, Tool, AgentID, Status string
		Arguments                         struct{ Name, AgentID, Task string }
	}
	if json.Unmarshal(line, &event) != nil {
		return
	}
	if event.Type != "invoke_subagent" && event.Name != "invoke_subagent" && event.Tool != "invoke_subagent" {
		return
	}
	name := event.Arguments.Name
	if name == "" {
		name = event.Arguments.AgentID
	}
	if name == "" {
		name = event.AgentID
	}
	if name == "" {
		name = "subagent"
	}
	status := event.Status
	if status == "" {
		status = "started"
	}
	w.subagentIndex++
	text := strings.TrimSpace(fmt.Sprintf("%s: %s\n%s", name, status, event.Arguments.Task))
	w.sink.Message(chat.Message{ID: fmt.Sprintf("%s-subagent-%d", w.sessionID, w.subagentIndex), SessionID: w.sessionID, Role: protocol.RoleSystem, Kind: "subagent", Text: text, Rev: 1, Timestamp: protocol.NowMillis()})
}

func (w *artifactWatcher) Close() { w.once.Do(func() { w.cancel(); <-w.done }) }

type watchedSession struct {
	types.Session
	watcher *artifactWatcher
}

func (s *watchedSession) Close() error { err := s.Session.Close(); s.watcher.Close(); return err }

type watchedSink struct {
	types.Sink
	watcher *artifactWatcher
}

func (s *watchedSink) Exit(code int, signal string) { s.watcher.Close(); s.Sink.Exit(code, signal) }

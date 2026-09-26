package opencode

import (
	"bufio"
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/process"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

var ports = struct {
	sync.Mutex
	used map[int]bool
}{used: make(map[int]bool)}

func reservePort() (int, error) {
	ports.Lock()
	defer ports.Unlock()
	for port := 14097; port <= 14200; port++ {
		if ports.used[port] {
			continue
		}
		listener, err := net.Listen("tcp", net.JoinHostPort("127.0.0.1", strconv.Itoa(port)))
		if err != nil {
			continue
		}
		_ = listener.Close()
		ports.used[port] = true
		return port, nil
	}
	return 0, fmt.Errorf("no available OpenCode port in 14097–14200")
}
func releasePort(port int) { ports.Lock(); delete(ports.used, port); ports.Unlock() }

type questionGroup struct {
	requestID string
	answers   [][]string
	remaining int
}
type pendingQuestion struct {
	group *questionGroup
	index int
}
type httpSession struct {
	mu                                      sync.Mutex
	cfg                                     types.SessionConfig
	sink                                    types.Sink
	ctx                                     context.Context
	cancel                                  context.CancelFunc
	baseURL, password, directory, sessionID string
	http                                    *http.Client
	cmd                                     *exec.Cmd
	done                                    chan struct{}
	parts                                   map[string]chat.Message
	partOwners                              map[string]string
	roles                                   map[string]protocol.ChatRole
	approvals                               map[string]string
	questions                               map[string]pendingQuestion
}

func newHTTPSession(ctx context.Context, cfg types.SessionConfig, sink types.Sink) *httpSession {
	ctx, cancel := context.WithCancel(ctx)
	dir := cfg.CWD
	if cfg.WorktreePath != "" {
		dir = cfg.WorktreePath
	}
	return &httpSession{ctx: ctx, cancel: cancel, cfg: cfg, sink: sink, directory: dir, http: &http.Client{}, done: make(chan struct{}), parts: make(map[string]chat.Message), partOwners: make(map[string]string), roles: make(map[string]protocol.ChatRole), approvals: make(map[string]string), questions: make(map[string]pendingQuestion)}
}

func startHTTPServer(ctx context.Context, bin string, cfg types.SessionConfig, sink types.Sink) (*httpSession, error) {
	s := newHTTPSession(ctx, cfg, sink)
	port, err := reservePort()
	if err != nil {
		s.cancel()
		return nil, err
	}
	secret := make([]byte, 32)
	if _, err := rand.Read(secret); err != nil {
		releasePort(port)
		s.cancel()
		return nil, err
	}
	s.password = hex.EncodeToString(secret)
	s.baseURL = "http://127.0.0.1:" + strconv.Itoa(port)
	cmd, err := process.CommandContext(s.ctx, bin, "serve", "--hostname", "127.0.0.1", "--port", strconv.Itoa(port))
	if err != nil {
		releasePort(port)
		s.cancel()
		return nil, err
	}
	cmd.Dir = s.directory
	cmd.Env = os.Environ()
	for key, value := range cfg.Env {
		cmd.Env = append(cmd.Env, key+"="+value)
	}
	cmd.Env = append(cmd.Env, "OPENCODE_SERVER_USERNAME=openremote", "OPENCODE_SERVER_PASSWORD="+s.password)
	cmd.Stdout, cmd.Stderr = io.Discard, io.Discard
	if err := cmd.Start(); err != nil {
		releasePort(port)
		s.cancel()
		return nil, err
	}
	s.cmd = cmd
	go func() {
		err := cmd.Wait()
		releasePort(port)
		code := 0
		if err != nil {
			code = cmd.ProcessState.ExitCode()
		}
		s.cancel()
		sink.Exit(code, "")
		close(s.done)
	}()
	startup, stop := context.WithTimeout(s.ctx, 25*time.Second)
	defer stop()
	ticker := time.NewTicker(150 * time.Millisecond)
	defer ticker.Stop()
	for {
		if err := s.request(startup, http.MethodGet, "/global/health", nil, nil); err == nil {
			break
		}
		select {
		case <-startup.Done():
			_ = s.Close()
			return nil, fmt.Errorf("OpenCode server did not become ready: %w", startup.Err())
		case <-ticker.C:
		}
	}
	var session struct {
		ID string `json:"id"`
	}
	if err := s.request(startup, http.MethodPost, "/session", map[string]any{"title": cfg.TaskName, "permission": []map[string]any{{"permission": "*", "pattern": "*", "action": "ask"}}}, &session); err != nil {
		_ = s.Close()
		return nil, err
	}
	if session.ID == "" {
		_ = s.Close()
		return nil, fmt.Errorf("OpenCode returned no session ID")
	}
	s.sessionID = session.ID
	response, err := s.openEvents()
	if err != nil {
		_ = s.Close()
		return nil, err
	}
	go func() {
		defer func() { _ = response.Body.Close() }()
		_ = consumeSSE(response.Body, s.handleEvent)
		// A lost native event stream cannot safely leave an agent awaiting
		// an approval the daemon never received. Contain that session.
		if s.ctx.Err() == nil {
			s.cancel()
		}
	}()
	return s, nil
}

func (s *httpSession) makeRequest(ctx context.Context, method, path string, body io.Reader) (*http.Request, error) {
	req, err := http.NewRequestWithContext(ctx, method, s.baseURL+path, body)
	if err != nil {
		return nil, err
	}
	req.SetBasicAuth("openremote", s.password)
	req.Header.Set("x-opencode-directory", s.directory)
	req.Header.Set("Content-Type", "application/json")
	return req, nil
}
func (s *httpSession) request(ctx context.Context, method, path string, body, result any) error {
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	var payload []byte
	if body != nil {
		var err error
		payload, err = json.Marshal(body)
		if err != nil {
			return err
		}
	}
	req, err := s.makeRequest(ctx, method, path, bytes.NewReader(payload))
	if err != nil {
		return err
	}
	response, err := s.http.Do(req)
	if err != nil {
		return err
	}
	defer func() { _ = response.Body.Close() }()
	if response.StatusCode >= 300 {
		data, _ := io.ReadAll(io.LimitReader(response.Body, 2048))
		return fmt.Errorf("OpenCode %s: HTTP %d: %s", path, response.StatusCode, strings.TrimSpace(string(data)))
	}
	if result == nil {
		_, _ = io.Copy(io.Discard, response.Body)
		return nil
	}
	return json.NewDecoder(io.LimitReader(response.Body, 16*1024*1024)).Decode(result)
}
func (s *httpSession) openEvents() (*http.Response, error) {
	req, err := s.makeRequest(s.ctx, http.MethodGet, "/event", nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "text/event-stream")
	response, err := s.http.Do(req)
	if err != nil {
		return nil, err
	}
	if response.StatusCode != http.StatusOK {
		_ = response.Body.Close()
		return nil, fmt.Errorf("OpenCode event stream: HTTP %d", response.StatusCode)
	}
	return response, nil
}

func consumeSSE(reader io.Reader, handle func([]byte)) error {
	scanner := bufio.NewScanner(reader)
	scanner.Buffer(make([]byte, 8192), 4*1024*1024)
	var data []string
	for scanner.Scan() {
		line := scanner.Text()
		if line == "" {
			if len(data) > 0 {
				handle([]byte(strings.Join(data, "\n")))
				data = nil
			}
			continue
		}
		if strings.HasPrefix(line, "data:") {
			data = append(data, strings.TrimPrefix(strings.TrimPrefix(line, "data:"), " "))
		}
	}
	if len(data) > 0 {
		handle([]byte(strings.Join(data, "\n")))
	}
	return scanner.Err()
}

func (s *httpSession) base() protocol.BaseEvent {
	return protocol.BaseEvent{SessionID: s.cfg.SessionID, Timestamp: protocol.NowMillis()}
}
func (s *httpSession) handleEvent(data []byte) {
	var event struct {
		Type       string          `json:"type"`
		Properties json.RawMessage `json:"properties"`
	}
	if err := json.Unmarshal(data, &event); err != nil {
		return
	}
	var p struct {
		ID, SessionID, MessageID, PartID, Field, Delta, Permission string
		Patterns                                                   []string
		Info                                                       struct {
			ID, SessionID string
			Role          protocol.ChatRole
			Time          struct{ Completed int64 }
		}
		Part struct {
			ID, SessionID, MessageID, Type, Text, Tool string
			Time                                       struct{ End int64 }
			State                                      struct {
				Status, Output, Error string
				Input                 map[string]any
			}
		}
		Questions []struct {
			Question string
			Options  []struct{ Label string }
			Multiple bool
		}
		Diff []struct {
			File, Patch, Before, After string
			Additions, Deletions       int
		}
		Error struct{ Data struct{ Message string } }
	}
	if err := json.Unmarshal(event.Properties, &p); err != nil {
		return
	}
	sessionID := p.SessionID
	if sessionID == "" {
		sessionID = p.Part.SessionID
	}
	if sessionID == "" {
		sessionID = p.Info.SessionID
	}
	if sessionID != s.sessionID {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	switch event.Type {
	case "message.updated":
		s.roles[p.Info.ID] = p.Info.Role
		if p.Info.Time.Completed > 0 {
			for id, owner := range s.partOwners {
				if owner == p.Info.ID {
					msg := s.parts[id]
					msg.Streaming = false
					msg.Rev++
					s.sink.Message(msg)
					delete(s.parts, id)
					delete(s.partOwners, id)
				}
			}
			delete(s.roles, p.Info.ID)
		}
	case "message.part.updated", "message.part.delta":
		part := p.Part
		id, owner := part.ID, part.MessageID
		if event.Type == "message.part.delta" {
			id, owner = p.PartID, p.MessageID
			if p.Field != "text" {
				return
			}
		}
		msg := s.parts[id]
		msg.ID, msg.SessionID, msg.Timestamp, msg.Rev = id, s.cfg.SessionID, protocol.NowMillis(), msg.Rev+1
		msg.Role = s.roles[owner]
		if msg.Role == "" {
			msg.Role = protocol.RoleAssistant
		}
		msg.Kind, msg.Streaming = "text", true
		if event.Type == "message.part.delta" {
			msg.Text += p.Delta
		} else {
			switch part.Type {
			case "text", "reasoning":
				msg.Text = part.Text
				msg.Streaming = part.Time.End == 0
				if part.Type == "reasoning" {
					msg.Kind = "thought"
				}
			case "tool":
				msg.Role, msg.ToolName, msg.Kind = protocol.RoleTool, part.Tool, "tool_result"
				msg.Text = part.State.Output
				if part.State.Error != "" {
					msg.Text = part.State.Error
				}
				msg.Streaming = part.State.Status != "completed" && part.State.Status != "error"
			default:
				return
			}
		}
		s.parts[id], s.partOwners[id] = msg, owner
		s.sink.Message(msg)
	case "permission.asked":
		id := s.cfg.SessionID + "-" + p.ID
		s.approvals[id] = p.ID
		s.sink.Event(protocol.ApprovalRequestedEvent{BaseEvent: s.base(), Type: protocol.EventApprovalRequested, ApprovalID: id, ToolName: p.Permission, Command: strings.Join(p.Patterns, "\n"), AutoDenyTimeoutMs: 120000})
	case "question.asked":
		group := &questionGroup{requestID: p.ID, answers: make([][]string, len(p.Questions)), remaining: len(p.Questions)}
		for index, question := range p.Questions {
			id := fmt.Sprintf("%s-%s-%d", s.cfg.SessionID, p.ID, index)
			s.questions[id] = pendingQuestion{group: group, index: index}
			options := make([]string, 0, len(question.Options))
			for _, option := range question.Options {
				options = append(options, option.Label)
			}
			s.sink.Event(protocol.QuestionAskedEvent{BaseEvent: s.base(), Type: protocol.EventQuestionAsked, QuestionID: id, QuestionText: question.Question, Options: options, IsMultiSelect: question.Multiple})
		}
	case "session.diff":
		for _, diff := range p.Diff {
			if diff.Patch == "" && diff.Before != diff.After {
				diff.Patch = replacementPatch(diff.File, diff.Before, diff.After)
			}
			s.sink.Event(protocol.DiffGeneratedEvent{BaseEvent: s.base(), Type: protocol.EventDiffGenerated, FilePath: diff.File, DiffPatch: diff.Patch, Additions: diff.Additions, Deletions: diff.Deletions})
		}
	case "session.idle":
		s.sink.Event(protocol.TurnCompletedEvent{BaseEvent: s.base(), Type: protocol.EventTurnCompleted})
	case "session.error":
		s.sink.Event(protocol.TurnCompletedEvent{BaseEvent: s.base(), Type: protocol.EventTurnCompleted, Summary: &p.Error.Data.Message})
	}
}

// OpenCode emits file contents rather than a patch in its current SSE schema.
// A full replacement hunk preserves the exact change without shelling out.
func replacementPatch(file, before, after string) string {
	lines := func(text string) []string {
		if text == "" {
			return nil
		}
		return strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	}
	old, updated := lines(before), lines(after)
	oldStart, newStart := 1, 1
	if len(old) == 0 {
		oldStart = 0
	}
	if len(updated) == 0 {
		newStart = 0
	}
	var patch strings.Builder
	fmt.Fprintf(&patch, "--- a/%s\n+++ b/%s\n@@ -%d,%d +%d,%d @@\n", file, file, oldStart, len(old), newStart, len(updated))
	for _, part := range []struct {
		prefix, text string
		lines        []string
	}{{"-", before, old}, {"+", after, updated}} {
		for _, line := range part.lines {
			fmt.Fprintf(&patch, "%s%s\n", part.prefix, line)
		}
		if part.text != "" && !strings.HasSuffix(part.text, "\n") {
			patch.WriteString("\\ No newline at end of file\n")
		}
	}
	return patch.String()
}

func (s *httpSession) Prompt(text string) error {
	if strings.TrimSpace(text) == "" {
		return fmt.Errorf("prompt is empty")
	}
	return s.request(s.ctx, http.MethodPost, "/session/"+url.PathEscape(s.sessionID)+"/prompt_async", map[string]any{"parts": []map[string]any{{"type": "text", "text": text}}}, nil)
}
func (s *httpSession) Approve(id string, approved bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	requestID, ok := s.approvals[id]
	if !ok {
		return fmt.Errorf("unknown approval %q", id)
	}
	reply := "reject"
	if approved {
		reply = "once"
	}
	if err := s.request(s.ctx, http.MethodPost, "/permission/"+url.PathEscape(requestID)+"/reply", map[string]any{"reply": reply}, nil); err != nil {
		return err
	}
	delete(s.approvals, id)
	return nil
}
func (s *httpSession) Answer(id string, answer any) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	q, ok := s.questions[id]
	if !ok {
		return fmt.Errorf("unknown question %q", id)
	}
	answers := []string{}
	switch value := answer.(type) {
	case []any:
		for _, part := range value {
			answers = append(answers, fmt.Sprint(part))
		}
	case []string:
		answers = value
	default:
		answers = append(answers, fmt.Sprint(value))
	}
	q.group.answers[q.index] = answers
	if q.group.remaining == 1 {
		if err := s.request(s.ctx, http.MethodPost, "/question/"+url.PathEscape(q.group.requestID)+"/reply", map[string]any{"answers": q.group.answers}, nil); err != nil {
			return err
		}
	}
	q.group.remaining--
	delete(s.questions, id)
	return nil
}
func (s *httpSession) RawInput([]byte) error {
	return fmt.Errorf("OpenCode server uses structured prompts")
}
func (s *httpSession) Resize(int, int) error { return nil }
func (s *httpSession) Snapshot() []byte      { return nil }
func (s *httpSession) Close() error {
	s.cancel()
	if s.cmd != nil {
		<-s.done
	}
	s.http.CloseIdleConnections()
	return nil
}

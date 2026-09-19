package codex

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/transport"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type approvalRequest struct{ id json.RawMessage }
type inputRequest struct {
	id        json.RawMessage
	answers   map[string]any
	remaining int
}
type inputQuestion struct {
	request *inputRequest
	key     string
}

type appSession struct {
	mu        sync.Mutex
	promptMu  sync.Mutex
	cfg       types.SessionConfig
	sink      types.Sink
	client    *transport.Client
	threadID  string
	ctx       context.Context
	cancel    context.CancelFunc
	items     map[string]chat.Message
	approvals map[string]approvalRequest
	questions map[string]inputQuestion
}

func startAppServer(ctx context.Context, bin string, cfg types.SessionConfig, sink types.Sink) (*appSession, error) {
	ctx, cancel := context.WithCancel(ctx)
	s := &appSession{cfg: cfg, sink: sink, ctx: ctx, cancel: cancel, items: make(map[string]chat.Message), approvals: make(map[string]approvalRequest), questions: make(map[string]inputQuestion)}
	cwd := cfg.CWD
	if cfg.WorktreePath != "" {
		cwd = cfg.WorktreePath
	}
	ready := make(chan struct{})
	client, err := transport.Start(ctx, bin, []string{"app-server", "--listen", "stdio://"}, cwd, cfg.Env,
		func(msg transport.Message) { <-ready; s.handle(msg) },
		func(data []byte) {
			for _, evt := range DetectAuthURL(cfg.SessionID, string(data)) {
				sink.Event(evt)
			}
		},
		func(code int) { sink.Exit(code, "") })
	if err != nil {
		cancel()
		return nil, err
	}
	s.client = client
	close(ready)
	startup, stop := context.WithTimeout(ctx, 30*time.Second)
	defer stop()
	if err := client.Call(startup, "initialize", map[string]any{
		"clientInfo":   map[string]any{"name": "openremote", "version": "0.1.0"},
		"capabilities": map[string]any{"experimentalApi": true},
	}, nil); err != nil {
		_ = s.Close()
		return nil, fmt.Errorf("codex initialize: %w", err)
	}
	if err := client.Notify("initialized", map[string]any{}); err != nil {
		_ = s.Close()
		return nil, err
	}
	var response struct {
		Thread struct {
			ID string `json:"id"`
		} `json:"thread"`
	}
	if err := client.Call(startup, "thread/start", map[string]any{"cwd": cwd, "approvalPolicy": "on-request", "sandbox": "workspace-write", "approvalsReviewer": "user"}, &response); err != nil {
		_ = s.Close()
		return nil, fmt.Errorf("codex thread/start: %w", err)
	}
	if response.Thread.ID == "" {
		_ = s.Close()
		return nil, fmt.Errorf("codex returned no thread ID")
	}
	s.threadID = response.Thread.ID
	return s, nil
}

func (s *appSession) base() protocol.BaseEvent {
	return protocol.BaseEvent{SessionID: s.cfg.SessionID, Timestamp: protocol.NowMillis()}
}

func (s *appSession) handle(msg transport.Message) {
	var params struct {
		ItemID  string `json:"itemId"`
		Delta   string `json:"delta"`
		Command string `json:"command"`
		Reason  string `json:"reason"`
		Diff    string `json:"diff"`
		Item    struct {
			ID      string `json:"id"`
			Type    string `json:"type"`
			Text    string `json:"text"`
			Command string `json:"command"`
			Output  string `json:"aggregatedOutput"`
			Content []struct {
				Text string `json:"text"`
			} `json:"content"`
			Changes []struct {
				Path string `json:"path"`
				Diff string `json:"diff"`
			} `json:"changes"`
		} `json:"item"`
		Questions []struct {
			ID       string `json:"id"`
			Question string `json:"question"`
			Options  []struct {
				Label string `json:"label"`
			} `json:"options"`
		} `json:"questions"`
		Turn struct {
			Status string `json:"status"`
			Error  *struct {
				Message string `json:"message"`
			} `json:"error"`
		} `json:"turn"`
	}
	if err := json.Unmarshal(msg.Params, &params); err != nil {
		return
	}
	if len(msg.ID) > 0 {
		switch msg.Method {
		case "item/commandExecution/requestApproval", "item/fileChange/requestApproval":
			id := s.cfg.SessionID + "-approval-" + base64.RawURLEncoding.EncodeToString(msg.ID)
			s.mu.Lock()
			s.approvals[id] = approvalRequest{id: msg.ID}
			s.mu.Unlock()
			tool, command := "command", params.Command
			if msg.Method == "item/fileChange/requestApproval" {
				tool, command = "fileChange", params.Reason
			}
			s.sink.Event(protocol.ApprovalRequestedEvent{BaseEvent: s.base(), Type: protocol.EventApprovalRequested, ApprovalID: id, ToolName: tool, Command: command, Description: &params.Reason, AutoDenyTimeoutMs: 120000})
		case "item/tool/requestUserInput":
			request := &inputRequest{id: msg.ID, answers: make(map[string]any), remaining: len(params.Questions)}
			if request.remaining == 0 {
				_ = s.client.Reply(msg.ID, map[string]any{"answers": request.answers})
				return
			}
			for _, q := range params.Questions {
				id := s.cfg.SessionID + "-question-" + base64.RawURLEncoding.EncodeToString(msg.ID) + "-" + base64.RawURLEncoding.EncodeToString([]byte(q.ID))
				s.mu.Lock()
				s.questions[id] = inputQuestion{request: request, key: q.ID}
				s.mu.Unlock()
				options := make([]string, 0, len(q.Options))
				for _, option := range q.Options {
					options = append(options, option.Label)
				}
				s.sink.Event(protocol.QuestionAskedEvent{BaseEvent: s.base(), Type: protocol.EventQuestionAsked, QuestionID: id, QuestionText: q.Question, Options: options})
			}
		default:
			_ = s.client.Send(map[string]any{"jsonrpc": "2.0", "id": msg.ID, "error": map[string]any{"code": -32601, "message": "Unsupported client request: " + msg.Method}})
		}
		return
	}
	switch msg.Method {
	case "item/agentMessage/delta":
		s.mu.Lock()
		item := s.items[params.ItemID]
		item.ID, item.SessionID, item.Role, item.Kind = params.ItemID, s.cfg.SessionID, protocol.RoleAssistant, "text"
		item.Text += params.Delta
		item.Rev++
		item.Streaming = true
		item.Timestamp = protocol.NowMillis()
		s.items[params.ItemID] = item
		s.mu.Unlock()
		s.sink.Message(item)
	case "item/completed":
		i := params.Item
		s.mu.Lock()
		item := s.items[i.ID]
		delete(s.items, i.ID)
		s.mu.Unlock()
		item.ID, item.SessionID, item.Timestamp, item.Streaming = i.ID, s.cfg.SessionID, protocol.NowMillis(), false
		item.Rev++
		item.Kind = "text"
		switch i.Type {
		case "agentMessage":
			item.Role, item.Text = protocol.RoleAssistant, i.Text
		case "userMessage":
			item.Role = protocol.RoleUser
			for _, part := range i.Content {
				item.Text += part.Text
			}
		case "commandExecution":
			item.Role, item.Kind, item.ToolName, item.Text = protocol.RoleTool, "tool_result", i.Command, i.Output
		case "fileChange":
			for _, change := range i.Changes {
				s.sink.Event(protocol.DiffGeneratedEvent{BaseEvent: s.base(), Type: protocol.EventDiffGenerated, FilePath: change.Path, DiffPatch: change.Diff})
			}
			return
		default:
			return
		}
		s.sink.Message(item)
	case "turn/completed":
		var summary *string
		if params.Turn.Error != nil {
			summary = &params.Turn.Error.Message
		}
		s.sink.Event(protocol.TurnCompletedEvent{BaseEvent: s.base(), Type: protocol.EventTurnCompleted, Summary: summary})
	}
}

func (s *appSession) Prompt(text string) error {
	if strings.TrimSpace(text) == "" {
		return fmt.Errorf("prompt is empty")
	}
	s.promptMu.Lock()
	defer s.promptMu.Unlock()
	ctx, cancel := context.WithTimeout(s.ctx, 30*time.Second)
	defer cancel()
	return s.client.Call(ctx, "turn/start", map[string]any{"threadId": s.threadID, "input": []map[string]any{{"type": "text", "text": text, "text_elements": []any{}}}}, nil)
}
func (s *appSession) Approve(id string, approved bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	request, ok := s.approvals[id]
	if !ok {
		return fmt.Errorf("unknown approval %q", id)
	}
	decision := "decline"
	if approved {
		decision = "accept"
	}
	if err := s.client.Reply(request.id, map[string]any{"decision": decision}); err != nil {
		return err
	}
	delete(s.approvals, id)
	return nil
}
func (s *appSession) Answer(id string, answer any) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	question, ok := s.questions[id]
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
	request := question.request
	request.answers[question.key] = map[string]any{"answers": answers}
	if request.remaining == 1 {
		if err := s.client.Reply(request.id, map[string]any{"answers": request.answers}); err != nil {
			return err
		}
	}
	request.remaining--
	delete(s.questions, id)
	return nil
}
func (s *appSession) RawInput([]byte) error {
	return fmt.Errorf("Codex app-server uses structured prompts; use a shell session for terminal input")
}
func (s *appSession) Resize(int, int) error { return nil }
func (s *appSession) Snapshot() []byte      { return nil }
func (s *appSession) Close() error          { err := s.client.Close(); s.cancel(); return err }

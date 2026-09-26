package pi

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/transport"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type rpcSession struct {
	mu           sync.Mutex
	ctx          context.Context
	cancel       context.CancelFunc
	cfg          types.SessionConfig
	sink         types.Sink
	client       *transport.Client
	message      chat.Message
	messageIndex int
	blocks       map[int]string
	requests     map[string]string
}

func decodeRPC(data []byte) (transport.Message, error) {
	var envelope struct {
		ID      json.RawMessage
		Type    string
		Success bool
		Error   string
		Data    json.RawMessage
	}
	if err := json.Unmarshal(data, &envelope); err != nil {
		return transport.Message{}, err
	}
	if envelope.Type != "response" {
		return transport.Message{Method: envelope.Type, Params: append(json.RawMessage(nil), data...)}, nil
	}
	msg := transport.Message{ID: envelope.ID, Result: envelope.Data}
	if len(msg.Result) == 0 {
		msg.Result = json.RawMessage("null")
	}
	if !envelope.Success {
		msg.Error = &transport.Error{Code: -1, Message: envelope.Error}
	}
	return msg, nil
}

func startRPC(ctx context.Context, bin string, cfg types.SessionConfig, sink types.Sink) (*rpcSession, error) {
	ctx, cancel := context.WithCancel(ctx)
	s := &rpcSession{ctx: ctx, cancel: cancel, cfg: cfg, sink: sink, blocks: make(map[int]string), requests: make(map[string]string)}
	cwd := cfg.CWD
	if cfg.WorktreePath != "" {
		cwd = cfg.WorktreePath
	}
	ready := make(chan struct{})
	client, err := transport.StartDecoded(ctx, bin, []string{"--mode", "rpc"}, cwd, cfg.Env,
		func(msg transport.Message) { <-ready; s.handle(msg) }, nil, func(code int) { sink.Exit(code, "") }, decodeRPC)
	if err != nil {
		cancel()
		return nil, err
	}
	s.client = client
	close(ready)
	probe, stop := context.WithTimeout(ctx, 20*time.Second)
	defer stop()
	if err := client.CallCommand(probe, map[string]any{"type": "get_state"}, nil); err != nil {
		_ = s.Close()
		return nil, fmt.Errorf("Pi RPC capability probe failed: %w", err)
	}
	return s, nil
}
func (s *rpcSession) base() protocol.BaseEvent {
	return protocol.BaseEvent{SessionID: s.cfg.SessionID, Timestamp: protocol.NowMillis()}
}

func contentText(data json.RawMessage) string {
	var text string
	if json.Unmarshal(data, &text) == nil {
		return text
	}
	var blocks []struct{ Type, Text, Thinking string }
	if json.Unmarshal(data, &blocks) != nil {
		return ""
	}
	var parts []string
	for _, block := range blocks {
		if block.Type == "text" {
			parts = append(parts, block.Text)
		}
	}
	return strings.Join(parts, "\n")
}

func (s *rpcSession) handle(msg transport.Message) {
	var event struct {
		ID, Method, Title, MessageText string
		Message                        json.RawMessage
		Options                        []string
		Timeout                        int
		AssistantMessageEvent          struct {
			Type         string
			ContentIndex int
			Delta        string
		}
		ToolCallID, ToolName  string
		Result, PartialResult struct{ Content json.RawMessage }
	}
	if err := json.Unmarshal(msg.Params, &event); err != nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	switch msg.Method {
	case "message_start":
		s.messageIndex++
		s.message = chat.Message{ID: fmt.Sprintf("%s-message-%d", s.cfg.SessionID, s.messageIndex), SessionID: s.cfg.SessionID, Role: protocol.RoleAssistant, Kind: "text", Streaming: true, Timestamp: protocol.NowMillis()}
		s.blocks = make(map[int]string)
	case "message_update":
		delta := event.AssistantMessageEvent
		if delta.Type != "text_delta" {
			return
		}
		s.blocks[delta.ContentIndex] += delta.Delta
		indices := make([]int, 0, len(s.blocks))
		for index := range s.blocks {
			indices = append(indices, index)
		}
		sort.Ints(indices)
		var text []string
		for _, index := range indices {
			text = append(text, s.blocks[index])
		}
		s.message.Text = strings.Join(text, "\n")
		s.message.Rev++
		s.sink.Message(s.message)
	case "message_end":
		var message struct {
			Role         string
			Content      json.RawMessage
			ToolName     string
			ToolCallID   string
			StopReason   string
			ErrorMessage string
		}
		if err := json.Unmarshal(event.Message, &message); err != nil {
			return
		}
		s.message.Text, s.message.Streaming = contentText(message.Content), false
		s.message.Role = protocol.ChatRole(message.Role)
		switch message.Role {
		case "system":
			return // the preamble carries structured sections, so its text field is empty
		case "toolResult":
			return // tool_execution_end already carries the result
		}
		// A failed turn still ends its assistant message. Surface the reason
		// instead of leaving an empty card.
		if message.StopReason == "error" && message.ErrorMessage != "" {
			s.message.Kind = "error"
			s.message.Text = strings.TrimSpace(s.message.Text + "\n\n" + message.ErrorMessage)
		}
		s.message.Rev++
		s.sink.Message(s.message)
	case "tool_execution_end", "tool_execution_update":
		result := event.Result
		if msg.Method == "tool_execution_update" {
			result = event.PartialResult
		}
		s.sink.Message(chat.Message{ID: event.ToolCallID, SessionID: s.cfg.SessionID, Role: protocol.RoleTool, Kind: "tool_result", ToolName: event.ToolName, Text: contentText(result.Content), Timestamp: protocol.NowMillis(), Rev: int(protocol.NowMillis()), Streaming: msg.Method != "tool_execution_end"})
	case "agent_end":
		s.sink.Event(protocol.TurnCompletedEvent{BaseEvent: s.base(), Type: protocol.EventTurnCompleted})
	case "extension_ui_request":
		id := s.cfg.SessionID + "-" + base64.RawURLEncoding.EncodeToString([]byte(event.ID))
		switch event.Method {
		case "confirm":
			s.requests[id] = event.ID
			var body struct{ Message string }
			_ = json.Unmarshal(msg.Params, &body)
			s.sink.Event(protocol.ApprovalRequestedEvent{BaseEvent: s.base(), Type: protocol.EventApprovalRequested, ApprovalID: id, ToolName: "Pi extension", Command: event.Title, Description: &body.Message, AutoDenyTimeoutMs: event.Timeout})
		case "select", "input", "editor":
			s.requests[id] = event.ID
			s.sink.Event(protocol.QuestionAskedEvent{BaseEvent: s.base(), Type: protocol.EventQuestionAsked, QuestionID: id, QuestionText: event.Title, Options: event.Options})
		}
	}
}

func (s *rpcSession) Prompt(text string) error {
	if strings.TrimSpace(text) == "" {
		return fmt.Errorf("prompt is empty")
	}
	ctx, cancel := context.WithTimeout(s.ctx, 30*time.Second)
	defer cancel()
	return s.client.CallCommand(ctx, map[string]any{"type": "prompt", "message": text, "streamingBehavior": "followUp"}, nil)
}
func (s *rpcSession) reply(id string, payload map[string]any) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	remoteID, ok := s.requests[id]
	if !ok {
		return fmt.Errorf("unknown Pi request %q", id)
	}
	payload["type"], payload["id"] = "extension_ui_response", remoteID
	if err := s.client.Send(payload); err != nil {
		return err
	}
	delete(s.requests, id)
	return nil
}
func (s *rpcSession) Approve(id string, approved bool) error {
	return s.reply(id, map[string]any{"confirmed": approved})
}
func (s *rpcSession) Answer(id string, answer any) error {
	return s.reply(id, map[string]any{"value": fmt.Sprint(answer)})
}
func (s *rpcSession) RawInput([]byte) error { return fmt.Errorf("Pi RPC uses structured prompts") }
func (s *rpcSession) Resize(int, int) error { return nil }
func (s *rpcSession) Snapshot() []byte      { return nil }
func (s *rpcSession) Close() error          { err := s.client.Close(); s.cancel(); return err }

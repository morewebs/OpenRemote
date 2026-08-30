package protocol

import (
	"encoding/json"
	"fmt"
	"time"
)

// AgentID — union of supported agent drivers
type AgentID string

const (
	AgentClaude      AgentID = "claude-code"
	AgentAntigravity AgentID = "antigravity"
	AgentOpenCode    AgentID = "opencode"
	AgentCodex       AgentID = "codex"
	AgentPi          AgentID = "pi"
	AgentShell       AgentID = "shell" // fallback / testing shell
)

func (a AgentID) Valid() bool {
	switch a {
	case AgentClaude, AgentAntigravity, AgentOpenCode, AgentCodex, AgentPi, AgentShell:
		return true
	}
	return false
}

// Base fields shared by every monotonic event (seq assigned by SQLite AUTOINCREMENT).
type BaseEvent struct {
	Seq       int64  `json:"seq"`
	SessionID string `json:"sessionId"`
	Timestamp int64  `json:"timestamp"` // unix-millis
}

type EventType string

const (
	EventStreamChunk       EventType = "stream.chunk"
	EventChatMessage       EventType = "chat.message"
	EventApprovalRequested EventType = "approval.requested"
	EventApprovalResolved  EventType = "approval.resolved"
	EventQuestionAsked     EventType = "question.asked"
	EventQuestionAnswered  EventType = "question.answered"
	EventDiffGenerated     EventType = "diff.generated"
	EventTurnCompleted     EventType = "turn.completed"
	EventAuthURL           EventType = "auth.url"
	EventSessionStatus     EventType = "session.status"
	EventArtifactUpdated   EventType = "artifact.updated"
)

type StreamChunkEvent struct {
	BaseEvent
	Type  EventType `json:"type"`
	Chunk string    `json:"chunk"` // base64-or-plain; JSON channel uses string
}

type ChatRole string

const (
	RoleUser      ChatRole = "user"
	RoleAssistant ChatRole = "assistant"
	RoleTool      ChatRole = "tool"
	RoleSystem    ChatRole = "system"
)

type ChatMessageEvent struct {
	BaseEvent
	Type      EventType      `json:"type"` // "chat.message"
	MessageID string         `json:"messageId"`
	Role      ChatRole       `json:"role"`
	Kind      string         `json:"kind"` // "text", "tool_use", "tool_result", "thought"
	Text      string         `json:"text"`
	ToolName  string         `json:"toolName,omitempty"`
	Meta      map[string]any `json:"meta,omitempty"`
	Streaming bool           `json:"streaming"`
	Rev       int            `json:"rev"`
}

type ApprovalRequestedEvent struct {
	BaseEvent
	Type              EventType `json:"type"`
	ApprovalID        string    `json:"approvalId"`
	ToolName          string    `json:"toolName"`
	Command           string    `json:"command"`
	Description       *string   `json:"description,omitempty"`
	AutoDenyTimeoutMs int       `json:"autoDenyTimeoutMs"`
}

type ApprovalResolvedEvent struct {
	BaseEvent
	Type       EventType `json:"type"`
	ApprovalID string    `json:"approvalId"`
	Approved   bool      `json:"approved"`
	ResolvedBy string    `json:"resolvedBy,omitempty"` // "user", "timeout", "auto"
}

type QuestionAskedEvent struct {
	BaseEvent
	Type          EventType `json:"type"`
	QuestionID    string    `json:"questionId"`
	QuestionText  string    `json:"questionText"`
	Options       []string  `json:"options"`
	IsMultiSelect bool      `json:"isMultiSelect"`
}

type QuestionAnsweredEvent struct {
	BaseEvent
	Type       EventType `json:"type"`
	QuestionID string    `json:"questionId"`
	Answers    []any     `json:"answers"`
}

type DiffGeneratedEvent struct {
	BaseEvent
	Type      EventType `json:"type"`
	FilePath  string    `json:"filePath"`
	DiffPatch string    `json:"diffPatch"`
	Additions int       `json:"additions"`
	Deletions int       `json:"deletions"`
}

type TurnCompletedEvent struct {
	BaseEvent
	Type       EventType `json:"type"`
	Summary    *string   `json:"summary,omitempty"`
	CostUSD    *float64  `json:"costUsd,omitempty"`
	DurationMs int64     `json:"durationMs"`
}

type AuthURLEvent struct {
	BaseEvent
	Type EventType `json:"type"`
	URL  string    `json:"url"`
}

type SessionStatusEvent struct {
	BaseEvent
	Type   EventType     `json:"type"`
	Status SessionStatus `json:"status"`
	Reason string        `json:"reason,omitempty"`
}

type ArtifactUpdatedEvent struct {
	BaseEvent
	Type    EventType `json:"type"`
	Path    string    `json:"path"`
	Kind    string    `json:"kind"` // "plan", "diff", "file"
	Content string    `json:"content"`
}

// AgentEvent is the discriminated union — use UnmarshalEvent to decode.
type AgentEvent struct {
	Type      EventType       `json:"type"`
	Raw       json.RawMessage `json:"-"`
	Seq       int64           `json:"seq"`
	SessionID string          `json:"sessionId"`
	Timestamp int64           `json:"timestamp"`
}

func (e *AgentEvent) UnmarshalJSON(data []byte) error {
	var probe struct {
		Type      EventType `json:"type"`
		Seq       int64     `json:"seq"`
		SessionID string    `json:"sessionId"`
		Timestamp int64     `json:"timestamp"`
	}
	if err := json.Unmarshal(data, &probe); err != nil {
		return err
	}
	e.Type = probe.Type
	e.Seq = probe.Seq
	e.SessionID = probe.SessionID
	e.Timestamp = probe.Timestamp
	e.Raw = json.RawMessage(data)
	return nil
}

func UnmarshalEvent(data []byte) (any, error) {
	var probe struct {
		Type EventType `json:"type"`
	}
	if err := json.Unmarshal(data, &probe); err != nil {
		return nil, err
	}
	switch probe.Type {
	case EventStreamChunk:
		var e StreamChunkEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventChatMessage:
		var e ChatMessageEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventApprovalRequested:
		var e ApprovalRequestedEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventApprovalResolved:
		var e ApprovalResolvedEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventQuestionAsked:
		var e QuestionAskedEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventQuestionAnswered:
		var e QuestionAnsweredEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventDiffGenerated:
		var e DiffGeneratedEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventTurnCompleted:
		var e TurnCompletedEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventAuthURL:
		var e AuthURLEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventSessionStatus:
		var e SessionStatusEvent
		err := json.Unmarshal(data, &e)
		return e, err
	case EventArtifactUpdated:
		var e ArtifactUpdatedEvent
		err := json.Unmarshal(data, &e)
		return e, err
	default:
		return nil, fmt.Errorf("unknown event type %q", probe.Type)
	}
}

func NowMillis() int64 { return time.Now().UnixMilli() }

// Session models from REST API
type SessionStatus string

const (
	StatusRunning         SessionStatus = "running"
	StatusIdle            SessionStatus = "idle"
	StatusWaitingApproval SessionStatus = "waiting_approval"
	StatusStopped         SessionStatus = "stopped"
)

type CreateSessionRequest struct {
	AgentID       AgentID `json:"agentId"`
	CWD           string  `json:"cwd"`
	UseWorktree   bool    `json:"useWorktree"`
	TaskName      *string `json:"taskName,omitempty"`
	RemoteControl bool    `json:"remoteControl,omitempty"`
	Cols          int     `json:"cols"`
	Rows          int     `json:"rows"`
}

func (r *CreateSessionRequest) Validate() error {
	if !r.AgentID.Valid() {
		return fmt.Errorf("invalid agentId %q", r.AgentID)
	}
	if r.CWD == "" {
		return fmt.Errorf("cwd required")
	}
	if r.Cols <= 0 {
		r.Cols = 120
	}
	if r.Rows <= 0 {
		r.Rows = 30
	}
	return nil
}

type CreateSessionResponse struct {
	SessionID    string        `json:"sessionId"`
	WorkspaceID  string        `json:"workspaceId"`
	WorktreePath *string       `json:"worktreePath,omitempty"`
	Status       SessionStatus `json:"status"`
}

type HealthResponse struct {
	Status   string `json:"status"`
	Uptime   int64  `json:"uptime"`
	Sessions int    `json:"sessions"`
}

type ApprovalReply struct {
	Approved bool `json:"approved"`
}

type QuestionReply struct {
	Answers []any `json:"answers"`
}

type AgentInfo struct {
	ID           AgentID          `json:"id"`
	DisplayName  string           `json:"displayName"`
	Available    bool             `json:"available"`
	Reason       string           `json:"reason,omitempty"`
	Capabilities DriverCapability `json:"capabilities"`
}

type DriverCapability struct {
	SupportsTerminal   bool `json:"supportsTerminal"`
	SupportsChatNative bool `json:"supportsChatNative"`
	SupportsApproval   bool `json:"supportsApproval"`
	SupportsDiff       bool `json:"supportsDiff"`
}

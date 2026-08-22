package driver

import (
	"context"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

// Capabilities mirrors AgentCapabilities from spec 03.
type Capabilities struct {
	SupportsWorktrees     bool
	SupportsApprovals     bool
	SupportsDisambiguation bool
	SupportsDiffStreams   bool
	SupportsACP           bool
}

type SessionConfig struct {
	SessionID string
	AgentID   protocol.AgentID
	CWD       string
	WorktreePath string
	Cols      int
	Rows      int
	Env       map[string]string
}

type Handle struct {
	SessionID    string
	WorkspaceID  string
	WorktreePath string
	Status       protocol.SessionStatus
}

type Event struct {
	SessionID string
	Payload   map[string]any
	Seq       int64
}

type Driver interface {
	AgentID() protocol.AgentID
	DisplayName() string
	Capabilities() Capabilities

	StartSession(ctx context.Context, cfg SessionConfig) (Handle, error)
	StopSession(sessionID string) error
	SendPrompt(sessionID, prompt string) error
	SendRawInput(sessionID string, data []byte) error
	SendApproval(sessionID, approvalID string, approved bool) error
	SendAnswer(sessionID, questionID string, answer any) error
	ResizeViewport(sessionID string, cols, rows int) error

	// Stream channels — closed on StopSession
	StreamCh(sessionID string) <-chan []byte
	EventCh(sessionID string) <-chan Event
	ExitCh(sessionID string) <-chan int
}

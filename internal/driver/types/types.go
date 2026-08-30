package types

import (
	"context"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

// SessionConfig configures launching an agent session.
type SessionConfig struct {
	SessionID     string
	AgentID       protocol.AgentID
	CWD           string
	WorktreePath  string
	Cols          int
	Rows          int
	Env           map[string]string
	TaskName      string
	RemoteControl bool
}

// Sink receives events, raw bytes, and structured messages from a running agent session.
type Sink interface {
	Bytes(data []byte)
	Message(msg chat.Message)
	Event(evt any)
	Exit(code int, signal string)
}

// Driver defines the lifecycle and capabilities for an agent backend.
type Driver interface {
	AgentID() protocol.AgentID
	DisplayName() string
	Capabilities() protocol.DriverCapability
	Probe() error
	Start(ctx context.Context, cfg SessionConfig, sink Sink) (Session, error)
}

// Session represents an active session instance.
type Session interface {
	Prompt(text string) error
	RawInput(data []byte) error
	Resize(cols, rows int) error
	Snapshot() []byte
	Approve(approvalID string, approved bool) error
	Answer(questionID string, answer any) error
	Close() error
}

// Terminal is an optional interface implemented by PTY-backed sessions.
type Terminal interface {
	RawInput(data []byte) error
	Resize(cols, rows int) error
	Snapshot() []byte
}

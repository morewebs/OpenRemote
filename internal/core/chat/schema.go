package chat

import (
	"github.com/morewebs/OpenRemote/internal/protocol"
)

// Message represents a structured chat message in a session transcript.
type Message struct {
	ID        string            `json:"id"`
	SessionID string            `json:"sessionId"`
	Role      protocol.ChatRole `json:"role"` // "user", "assistant", "tool", "system"
	Kind      string            `json:"kind"` // "text", "tool_use", "tool_result", "thought"
	Text      string            `json:"text"`
	ToolName  string            `json:"toolName,omitempty"`
	Meta      map[string]any    `json:"meta,omitempty"`
	Streaming bool              `json:"streaming"`
	Rev       int               `json:"rev"`
	Timestamp int64             `json:"timestamp"`
	Seq       int64             `json:"seq,omitempty"`
}

// Lexer classifies a clean screen/scrollback line into a role and semantic kind.
type Lexer interface {
	// Classify analyzes a line of output.
	// Returns:
	// - role: target role (user, assistant, tool, system)
	// - kind: semantic kind ("text", "tool_use", "tool_result", "thought")
	// - isNewBlock: true if this line begins a new logical block/turn
	// - cleanText: line text with marker symbols stripped
	// - skip: true if this line is visual noise (spinner, box borders, hotkey hints)
	Classify(line string) (role protocol.ChatRole, kind string, isNewBlock bool, cleanText string, skip bool)
}

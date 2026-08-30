package chat

import (
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

// Assembler aggregates committed terminal lines into structured, streaming chat messages.
type Assembler struct {
	mu        sync.Mutex
	sessionID string
	lexer     Lexer
	onMessage func(msg Message)

	msgIndex   int
	currentMsg *Message
	lines      []string

	flushTimer *time.Timer
	idleDebounce time.Duration
}

// NewAssembler creates a new chat Assembler for a session.
func NewAssembler(sessionID string, lexer Lexer, onMessage func(msg Message)) *Assembler {
	if lexer == nil {
		lexer = NewGenericLexer()
	}
	return &Assembler{
		sessionID:    sessionID,
		lexer:        lexer,
		onMessage:    onMessage,
		idleDebounce: 600 * time.Millisecond,
	}
}

// Feed processes a single committed line of terminal output.
func (a *Assembler) Feed(line string) {
	a.mu.Lock()
	defer a.mu.Unlock()

	role, kind, isNewBlock, cleanText, skip := a.lexer.Classify(line)
	if skip {
		return
	}

	// If starting a new block or changing role/kind, seal previous message
	if isNewBlock || (a.currentMsg != nil && (a.currentMsg.Role != role || a.currentMsg.Kind != kind)) {
		a.sealCurrentLocked()
	}

	if a.currentMsg == nil {
		a.msgIndex++
		a.currentMsg = &Message{
			ID:        fmt.Sprintf("msg_%s_%d", a.sessionID, a.msgIndex),
			SessionID: a.sessionID,
			Role:      role,
			Kind:      kind,
			Streaming: true,
			Rev:       1,
			Timestamp: protocol.NowMillis(),
		}
		a.lines = nil
	}

	if cleanText != "" || len(a.lines) > 0 {
		a.lines = append(a.lines, cleanText)
		a.currentMsg.Text = strings.Join(a.lines, "\n")
		a.currentMsg.Rev++
		a.emitLocked(*a.currentMsg)
	}

	a.resetTimerLocked()
}

// Flush seals any currently pending in-flight message.
func (a *Assembler) Flush() {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.sealCurrentLocked()
}

func (a *Assembler) sealCurrentLocked() {
	if a.flushTimer != nil {
		a.flushTimer.Stop()
		a.flushTimer = nil
	}

	if a.currentMsg == nil {
		return
	}

	finalText := strings.TrimSpace(strings.Join(a.lines, "\n"))
	if finalText != "" {
		a.currentMsg.Text = finalText
		a.currentMsg.Streaming = false
		a.currentMsg.Rev++
		a.emitLocked(*a.currentMsg)
	}

	a.currentMsg = nil
	a.lines = nil
}

func (a *Assembler) emitLocked(msg Message) {
	if a.onMessage != nil {
		// Emit copy outside lock or via callback
		go a.onMessage(msg)
	}
}

func (a *Assembler) resetTimerLocked() {
	if a.flushTimer != nil {
		a.flushTimer.Stop()
	}
	a.flushTimer = time.AfterFunc(a.idleDebounce, func() {
		a.mu.Lock()
		defer a.mu.Unlock()
		if a.currentMsg != nil && a.currentMsg.Streaming {
			a.sealCurrentLocked()
		}
	})
}

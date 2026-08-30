package chat

import (
	"sync"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

func TestClaudeLexer(t *testing.T) {
	lexer := NewClaudeLexer()

	// Test user prompt
	role, kind, isNew, clean, skip := lexer.Classify("> What is 2 + 2?")
	if role != protocol.RoleUser || !isNew || clean != "What is 2 + 2?" || skip {
		t.Fatalf("Unexpected user prompt classification: role=%v, isNew=%v, clean=%q, skip=%v", role, isNew, clean, skip)
	}

	// Test assistant response
	role, kind, isNew, clean, skip = lexer.Classify("⏺ 2 + 2 is 4.")
	if role != protocol.RoleAssistant || !isNew || clean != "2 + 2 is 4." || skip {
		t.Fatalf("Unexpected assistant classification: role=%v, isNew=%v, clean=%q, skip=%v", role, isNew, clean, skip)
	}

	// Test tool use
	role, kind, isNew, clean, skip = lexer.Classify("⏺ Running bash...")
	if role != protocol.RoleTool || kind != "tool_use" || !isNew || skip {
		t.Fatalf("Unexpected tool classification: role=%v, kind=%v, isNew=%v, skip=%v", role, kind, isNew, skip)
	}

	// Test tool result
	role, kind, isNew, clean, skip = lexer.Classify("⎿ Result: 4")
	if role != protocol.RoleTool || kind != "tool_result" || clean != "Result: 4" || skip {
		t.Fatalf("Unexpected tool result classification: role=%v, kind=%v, clean=%q, skip=%v", role, kind, clean, skip)
	}

	// Test spinner / noise
	_, _, _, _, skip = lexer.Classify("⠋ Thinking...")
	if !skip {
		t.Fatalf("Expected spinner line to be skipped")
	}
}

func TestAssemblerStreaming(t *testing.T) {
	var mu sync.Mutex
	var messages []Message

	assembler := NewAssembler("sess_123", NewClaudeLexer(), func(msg Message) {
		mu.Lock()
		defer mu.Unlock()
		messages = append(messages, msg)
	})

	assembler.Feed("> hello")
	assembler.Feed("⏺ Hello! How can I help you today?")
	assembler.Feed("I am ready.")
	assembler.Flush()

	time.Sleep(50 * time.Millisecond)

	mu.Lock()
	defer mu.Unlock()

	if len(messages) == 0 {
		t.Fatalf("Expected assembled messages, got 0")
	}

	t.Logf("Received %d messages", len(messages))
}

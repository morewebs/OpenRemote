package chat

import (
	"strings"
	"unicode"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

// GenericLexer is a default fallback lexer for any terminal stream.
type GenericLexer struct{}

func NewGenericLexer() *GenericLexer {
	return &GenericLexer{}
}

func (l *GenericLexer) Classify(line string) (role protocol.ChatRole, kind string, isNewBlock bool, cleanText string, skip bool) {
	trimmed := strings.TrimSpace(line)
	if trimmed == "" {
		return protocol.RoleAssistant, "text", false, "", false
	}

	// Filter common TUI border noise
	if isBorderNoise(trimmed) {
		return protocol.RoleAssistant, "text", false, "", true
	}

	// Detect user prompt line
	if strings.HasPrefix(trimmed, "> ") || strings.HasPrefix(trimmed, "$ ") {
		clean := strings.TrimPrefix(strings.TrimPrefix(trimmed, "> "), "$ ")
		return protocol.RoleUser, "text", true, clean, false
	}

	return protocol.RoleAssistant, "text", false, line, false
}

// ClaudeLexer parses output lines from Claude Code.
type ClaudeLexer struct{}

func NewClaudeLexer() *ClaudeLexer {
	return &ClaudeLexer{}
}

func (l *ClaudeLexer) Classify(line string) (role protocol.ChatRole, kind string, isNewBlock bool, cleanText string, skip bool) {
	trimmed := strings.TrimSpace(line)
	if trimmed == "" {
		return protocol.RoleAssistant, "text", false, "", false
	}

	// Noise filtering: spinners, hotkey bars, esc to interrupt
	lower := strings.ToLower(trimmed)
	if strings.Contains(lower, "esc to interrupt") ||
		strings.Contains(lower, "ctrl+c to exit") ||
		strings.Contains(lower, "type /help for commands") ||
		isSpinnerNoise(trimmed) ||
		isBorderNoise(trimmed) {
		return protocol.RoleAssistant, "text", false, "", true
	}

	// User input echo: "> prompt" or "❯ prompt"
	if strings.HasPrefix(trimmed, "> ") || strings.HasPrefix(trimmed, "❯ ") {
		clean := strings.TrimSpace(trimmed[strings.Index(trimmed, " ")+1:])
		return protocol.RoleUser, "text", true, clean, false
	}

	// Tool call or Assistant block markers:
	// "⏺ " (U+23FA) or "● " (U+25CF) or "* "
	if strings.HasPrefix(trimmed, "⏺") || strings.HasPrefix(trimmed, "●") {
		clean := strings.TrimSpace(strings.TrimPrefix(strings.TrimPrefix(trimmed, "⏺"), "●"))
		// Check if this is a tool use header like "⏺ Reading file..." or "⏺ Running bash..."
		if strings.HasPrefix(clean, "Running ") || strings.HasPrefix(clean, "Reading ") || strings.HasPrefix(clean, "Editing ") || strings.HasPrefix(clean, "Searching ") {
			return protocol.RoleTool, "tool_use", true, clean, false
		}
		return protocol.RoleAssistant, "text", true, clean, false
	}

	// Tool result marker: "⎿ " (U+23BF) or "└ "
	if strings.HasPrefix(trimmed, "⎿") || strings.HasPrefix(trimmed, "└") {
		clean := strings.TrimSpace(strings.TrimPrefix(strings.TrimPrefix(trimmed, "⎿"), "└"))
		return protocol.RoleTool, "tool_result", false, clean, false
	}

	// Thought / reasoning markers
	if strings.HasPrefix(trimmed, "Thinking Process:") || strings.HasPrefix(trimmed, "💭") {
		clean := strings.TrimSpace(strings.TrimPrefix(trimmed, "💭"))
		return protocol.RoleAssistant, "thought", true, clean, false
	}

	return protocol.RoleAssistant, "text", false, line, false
}

func isBorderNoise(s string) bool {
	if len(s) == 0 {
		return false
	}
	borderRunes := "─━│┃┌┐└┘├┤┬┴┼╭╮╯╰═║╔╗╚╝╠╣╦╩╬"
	allBorder := true
	for _, r := range s {
		if !unicode.IsSpace(r) && !strings.ContainsRune(borderRunes, r) {
			allBorder = false
			break
		}
	}
	return allBorder
}

func isSpinnerNoise(s string) bool {
	spinnerRunes := "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
	for _, r := range spinnerRunes {
		if strings.ContainsRune(s, r) {
			return true
		}
	}
	return false
}

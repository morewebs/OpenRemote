package screen

import (
	"fmt"
	"strings"
	"testing"
)

func TestShortResponsesCommitBeforeScrollingAndOnlyOnce(t *testing.T) {
	s := New(80, 30)
	var lines []string
	s.OnCommit(func(line string) { lines = append(lines, line) })
	_, _ = s.Write([]byte("first response\r\n"))
	if len(lines) != 1 || lines[0] != "first response" {
		t.Fatalf("short response missing: %v", lines)
	}
	_, _ = s.Write([]byte("partial"))
	if len(lines) != 1 {
		t.Fatal("partial cursor row committed")
	}
	_, _ = s.Write([]byte(" response\r\n"))
	for i := 0; i < 80; i++ {
		_, _ = s.Write([]byte(fmt.Sprintf("line %d\r\n", i)))
	}
	s.FlushCurrentScreenLines()
	if len(lines) != 82 {
		t.Fatalf("expected each completed row once, got %d", len(lines))
	}
}

func TestChatContinuesPastScrollbackCapacity(t *testing.T) {
	s := New(40, 5)
	s.emu.SetScrollbackSize(3)
	count := 0
	s.OnCommit(func(string) { count++ })
	for i := 0; i < 30; i++ {
		_, _ = s.Write([]byte("same repeated line\r\n"))
	}
	s.FlushCurrentScreenLines()
	if count != 30 {
		t.Fatalf("got %d lines after capacity was exceeded", count)
	}
}

func TestScreenCommit(t *testing.T) {
	s := New(80, 5)

	var committed []string
	s.OnCommit(func(line string) {
		committed = append(committed, line)
	})

	// Write 10 lines of text with newlines to force scrolling
	for i := 1; i <= 10; i++ {
		_, err := s.Write([]byte(strings.Repeat("a", 10) + "\r\n"))
		if err != nil {
			t.Fatalf("Write error: %v", err)
		}
	}

	if len(committed) == 0 {
		t.Fatalf("Expected committed lines from scrolling, got 0")
	}

	t.Logf("Committed %d lines", len(committed))
}

func TestScreenAltScreenSuppression(t *testing.T) {
	s := New(80, 5)

	var committed []string
	s.OnCommit(func(line string) {
		committed = append(committed, line)
	})

	// Enter alt-screen: \x1b[?1049h
	_, _ = s.Write([]byte("\x1b[?1049h"))
	if !s.InAltScreen() {
		t.Fatalf("Expected InAltScreen to be true")
	}

	// Write lines in alt screen
	for i := 1; i <= 10; i++ {
		_, _ = s.Write([]byte("Alt screen line\r\n"))
	}

	if len(committed) > 0 {
		t.Fatalf("Expected 0 committed lines in alt screen, got %d", len(committed))
	}

	// Exit alt-screen: \x1b[?1049l
	_, _ = s.Write([]byte("\x1b[?1049l"))
	if s.InAltScreen() {
		t.Fatalf("Expected InAltScreen to be false")
	}
}

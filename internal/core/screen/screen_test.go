package screen

import (
	"strings"
	"testing"
)

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
	s.Write([]byte("\x1b[?1049h"))
	if !s.InAltScreen() {
		t.Fatalf("Expected InAltScreen to be true")
	}

	// Write lines in alt screen
	for i := 1; i <= 10; i++ {
		s.Write([]byte("Alt screen line\r\n"))
	}

	if len(committed) > 0 {
		t.Fatalf("Expected 0 committed lines in alt screen, got %d", len(committed))
	}

	// Exit alt-screen: \x1b[?1049l
	s.Write([]byte("\x1b[?1049l"))
	if s.InAltScreen() {
		t.Fatalf("Expected InAltScreen to be false")
	}
}

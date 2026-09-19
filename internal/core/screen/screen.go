package screen

import (
	"bytes"
	"strings"
	"sync"
	"time"

	"github.com/charmbracelet/x/vt"
)

// Screen wraps a virtual terminal emulator and produces clean, committed
// scrollback text lines for stateful chat extraction and parsing.
type Screen struct {
	mu            sync.Mutex
	emu           *vt.SafeEmulator
	cols          int
	rows          int
	scrolled      int // logical offset of the active viewport
	emitted       map[int]string
	onCommit      func(line string)
	altScreen     bool
	lastWriteTime time.Time
}

// New creates a new virtual Screen with the specified dimensions.
func New(cols, rows int) *Screen {
	if cols <= 0 {
		cols = 120
	}
	if rows <= 0 {
		rows = 30
	}
	return &Screen{
		emu:     vt.NewSafeEmulator(cols, rows),
		cols:    cols,
		rows:    rows,
		emitted: make(map[int]string),
	}
}

// OnCommit registers a callback invoked whenever a line is permanently committed to scrollback.
func (s *Screen) OnCommit(fn func(line string)) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.onCommit = fn
}

// InAltScreen returns true if the terminal is currently rendering an alternate screen buffer
// (e.g. full-screen interactive TUIs, nano, less, interactive pickers).
func (s *Screen) InAltScreen() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.emu.IsAltScreen()
}

// Resize updates the terminal emulator geometry.
func (s *Screen) Resize(cols, rows int) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if cols <= 0 {
		cols = 120
	}
	if rows <= 0 {
		rows = 30
	}
	s.cols = cols
	s.rows = rows
	s.emu.Resize(cols, rows)
}

// Write processes incoming raw terminal bytes through the VT engine and triggers commits.
func (s *Screen) Write(p []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	s.lastWriteTime = time.Now()
	n, err := s.emu.Write(p)
	s.altScreen = s.emu.IsAltScreen()

	// If in alternate screen mode, suppress scrollback line emissions
	if !s.altScreen {
		s.checkScrollbackCommits()
		// A completed line need not leave the viewport to be useful in chat.
		// Exclude the cursor row, which may still be a prompt or partial token.
		if bytes.ContainsRune(p, '\n') {
			s.commitVisible(s.emu.CursorPosition().Y - 1)
		}
	}

	return n, err
}

// checkScrollbackCommits emits any newly committed scrollback lines.
func (s *Screen) checkScrollbackCommits() {
	sbLen := s.emu.ScrollbackLen()
	if sbLen == 0 {
		return
	}

	for y := 0; y < sbLen; y++ {
		s.commitLine(s.scrolled+y, s.readScrollbackLine(y))
		delete(s.emitted, s.scrolled+y)
	}
	s.scrolled += sbLen
	// Consume committed rows so the VT library's capped scrollback cannot
	// stop our progress once it reaches its maximum length.
	s.emu.ClearScrollback()
}

func (s *Screen) commitLine(index int, line string) {
	if previous, ok := s.emitted[index]; ok && previous == line {
		return
	}
	s.emitted[index] = line
	if line != "" && s.onCommit != nil {
		s.onCommit(line)
	}
}

func (s *Screen) commitVisible(maxLine int) {
	if maxLine >= s.rows {
		maxLine = s.rows - 1
	}
	for y := 0; y <= maxLine; y++ {
		var line strings.Builder
		for x := 0; x < s.cols; x++ {
			cell := s.emu.CellAt(x, y)
			if cell != nil && cell.Content != "" {
				line.WriteString(cell.Content)
			} else {
				line.WriteByte(' ')
			}
		}
		s.commitLine(s.scrolled+y, strings.TrimRight(line.String(), " "))
	}
}

// readScrollbackLine converts a scrollback row to a clean trimmed string.
func (s *Screen) readScrollbackLine(y int) string {
	var sb strings.Builder
	for x := 0; x < s.cols; x++ {
		cell := s.emu.ScrollbackCellAt(x, y)
		if cell != nil && cell.Content != "" {
			sb.WriteString(cell.Content)
		} else {
			sb.WriteByte(' ')
		}
	}
	return strings.TrimRight(sb.String(), " ")
}

// ReadScreenLine reads an active row on the visible viewport (0 <= y < rows).
func (s *Screen) ReadScreenLine(y int) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	if y < 0 || y >= s.rows {
		return ""
	}
	var sb strings.Builder
	for x := 0; x < s.cols; x++ {
		cell := s.emu.CellAt(x, y)
		if cell != nil && cell.Content != "" {
			sb.WriteString(cell.Content)
		} else {
			sb.WriteByte(' ')
		}
	}
	return strings.TrimRight(sb.String(), " ")
}

// ActiveScreenLines returns all non-empty lines on the current visible screen.
func (s *Screen) ActiveScreenLines() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	var lines []string
	for y := 0; y < s.rows; y++ {
		var sb strings.Builder
		for x := 0; x < s.cols; x++ {
			cell := s.emu.CellAt(x, y)
			if cell != nil && cell.Content != "" {
				sb.WriteString(cell.Content)
			} else {
				sb.WriteByte(' ')
			}
		}
		line := strings.TrimRight(sb.String(), " ")
		lines = append(lines, line)
	}
	return lines
}

// FlushCurrentScreenLines commits all lines above the current cursor position or non-empty lines.
func (s *Screen) FlushCurrentScreenLines() {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.altScreen {
		return
	}

	// Emit any scrollback lines first
	s.checkScrollbackCommits()

	s.commitVisible(s.emu.CursorPosition().Y)
}

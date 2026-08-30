package screen

import (
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
	committedLine int // number of scrollback lines already emitted
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
		emu:  vt.NewSafeEmulator(cols, rows),
		cols: cols,
		rows: rows,
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
	}

	return n, err
}

// checkScrollbackCommits emits any newly committed scrollback lines.
func (s *Screen) checkScrollbackCommits() {
	sbLen := s.emu.ScrollbackLen()
	if sbLen <= s.committedLine {
		return
	}

	for y := s.committedLine; y < sbLen; y++ {
		line := s.readScrollbackLine(y)
		if s.onCommit != nil {
			s.onCommit(line)
		}
	}
	s.committedLine = sbLen
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

	// For any screen lines that contain text, emit them if we're flushing on exit
	pos := s.emu.CursorPosition()
	maxLine := pos.Y
	if maxLine >= s.rows {
		maxLine = s.rows - 1
	}

	for y := 0; y <= maxLine; y++ {
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
		if line != "" && s.onCommit != nil {
			s.onCommit(line)
		}
	}
}

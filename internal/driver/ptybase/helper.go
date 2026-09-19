package ptybase

import (
	"fmt"
	"os"
	"os/exec"
)

// FindBinary resolves an executable from an ordered set of candidate names and
// extra absolute install paths. For each candidate it first consults the OS PATH
// resolver (exec.LookPath) and then falls back to a direct os.Stat probe, so
// callers may pass install locations that are not on PATH (for example
// %APPDATA%/npm or ~/.npm-global/bin). The names slice is tried before the
// extraPaths slice and the first existing match wins. Callers should wrap the
// generic error with a driver-specific message to keep behaviour identical.
func FindBinary(names []string, extraPaths []string) (string, error) {
	candidates := make([]string, 0, len(names)+len(extraPaths))
	candidates = append(candidates, names...)
	candidates = append(candidates, extraPaths...)

	for _, cand := range candidates {
		if path, err := exec.LookPath(cand); err == nil {
			return path, nil
		}
		if fi, err := os.Stat(cand); err == nil && !fi.IsDir() {
			return cand, nil
		}
	}
	return "", fmt.Errorf("binary not found in PATH or standard install locations")
}

// TrimRightPunct strips trailing punctuation that terminal renderers may attach
// to the end of a captured URL so links stay clickable.
func TrimRightPunct(s string) string {
	for len(s) > 0 {
		switch s[len(s)-1] {
		case '.', ',', ';', ':', ')', ']', '>', '\'', '"':
			s = s[:len(s)-1]
		default:
			return s
		}
	}
	return s
}

// BracketedPaste wraps a prompt in bracketed-paste mode sequences so the CLI
// treats a multi-line prompt as a single atomic paste instead of executing
// line-by-line through the terminal line buffer.
func BracketedPaste(prompt string) []byte {
	return []byte("\x1b[200~" + prompt + "\x1b[201~\r\n")
}

// ChainLineHooks composes several line hooks into a single LineHook. Every
// hook sees the same committed screen line and all returned events are
// concatenated in order. Nil hooks are skipped. This keeps LineHook composition
// declarative — drivers expose one hook that is really a chain of detectors
// (login URLs, approval prompts, auth URLs, ...).
func ChainLineHooks(hooks ...func(sessionID, line string) []any) func(sessionID, line string) []any {
	return func(sessionID, line string) []any {
		var out []any
		for _, h := range hooks {
			if h == nil {
				continue
			}
			out = append(out, h(sessionID, line)...)
		}
		return out
	}
}

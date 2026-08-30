package claude

import (
	"regexp"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

// BracketedPaste wraps a prompt in bracketed paste mode sequences so the CLI
// treats a multi-line prompt as a single atomic paste instead of executing
// line-by-line through the terminal line buffer.
func BracketedPaste(prompt string) []byte {
	return []byte("\x1b[200~" + prompt + "\x1b[201~\r\n")
}

// reLoginURL matches OAuth device-flow login links printed by the CLI
// (e.g. https://claude.ai/oauth/authorize?... or https://claude.ai/login?...).
var reLoginURL = regexp.MustCompile(`https://claude\.ai/(?:oauth/authorize|login)\?[^\s"']+`)

// DetectLoginURL is a ptybase LineHook that emits an AuthURLEvent when the
// CLI renders an OAuth device-flow login link, letting remote clients show a
// clickable "Log in" action instead of raw terminal output.
func DetectLoginURL(sessionID, line string) []any {
	u := trimRightPunct(reLoginURL.FindString(line))
	if u == "" {
		return nil
	}
	return []any{protocol.AuthURLEvent{
		BaseEvent: protocol.BaseEvent{
			SessionID: sessionID,
			Timestamp: protocol.NowMillis(),
		},
		Type: protocol.EventAuthURL,
		URL:  u,
	}}
}

// trimRightPunct strips trailing punctuation that terminal renderers may
// attach to the end of a URL line.
func trimRightPunct(s string) string {
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

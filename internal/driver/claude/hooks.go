package claude

import (
	"regexp"

	"github.com/morewebs/OpenRemote/internal/driver/ptybase"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

// BracketedPaste wraps a prompt in bracketed paste mode sequences so the CLI
// treats a multi-line prompt as a single atomic paste instead of executing
// line-by-line through the terminal line buffer. It delegates to the shared
// ptybase implementation so all PTY drivers emit identical paste framing.
func BracketedPaste(prompt string) []byte {
	return ptybase.BracketedPaste(prompt)
}

// reLoginURL matches OAuth device-flow login links printed by the CLI
// (e.g. https://claude.ai/oauth/authorize?... or https://claude.ai/login?...).
var reLoginURL = regexp.MustCompile(`https://claude\.ai/(?:oauth/authorize|login)\?[^\s"']+`)

// DetectLoginURL is a ptybase LineHook that emits an AuthURLEvent when the
// CLI renders an OAuth device-flow login link, letting remote clients show a
// clickable "Log in" action instead of raw terminal output.
func DetectLoginURL(sessionID, line string) []any {
	u := ptybase.TrimRightPunct(reLoginURL.FindString(line))
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

// LineHook is the composed hook Claude feeds into ptybase: today it is the
// login-URL detector, but composition via ptybase.ChainLineHooks keeps the seam
// ready for additional detectors (approval prompts, turn completion, ...)
// without rewriting driver wiring.
//
// ApprovalRequestedEvent is deliberately NOT emitted from the driver hook:
// approvals are detected and registered in the approvals registry by the
// server-side parser path (see internal/core/parser). A driver-emitted approval
// would broadcast an event that the registry never recorded, so Approve RPCs
// would fail to resolve it. Keeping approval emission in one place avoids
// duplicate / phantom approvals.
var LineHook = ptybase.ChainLineHooks(DetectLoginURL)

package codex

import (
	"regexp"
	"strings"

	"github.com/morewebs/OpenRemote/internal/driver/ptybase"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

// reURL captures any https:// URL token from a committed screen line.
var reURL = regexp.MustCompile(`https?://[^\s"'<>)\]]+`)

// Detected hosts cover the browser/device-code flows Codex prints during
// sign-in: auth.openai.com, chatgpt.com/codex, platform.openai.com, etc.
var codexAuthHosts = []string{"auth.openai.com", "chatgpt.com", "platform.openai.com", "openai.com"}

// DetectAuthURL is a ptybase LineHook that emits an AuthURLEvent when Codex
// renders an OpenAI sign-in URL, so remote clients can surface a clickable
// "Log in" action instead of raw terminal text.
func DetectAuthURL(sessionID, line string) []any {
	for _, raw := range reURL.FindAllString(line, -1) {
		u := ptybase.TrimRightPunct(raw)
		if isCodexAuthURL(u) {
			return []any{protocol.AuthURLEvent{
				BaseEvent: protocol.BaseEvent{
					SessionID: sessionID,
					Timestamp: protocol.NowMillis(),
				},
				Type: protocol.EventAuthURL,
				URL:  u,
			}}
		}
	}
	return nil
}

func isCodexAuthURL(u string) bool {
	rest := u
	if i := strings.Index(rest, "://"); i >= 0 {
		rest = rest[i+3:]
	}
	if i := strings.IndexAny(rest, "/?#"); i >= 0 {
		rest = rest[:i]
	}
	host := strings.ToLower(rest)
	if at := strings.LastIndex(host, "@"); at >= 0 {
		host = host[at+1:]
	}
	if colon := strings.Index(host, ":"); colon >= 0 {
		host = host[:colon]
	}
	for _, h := range codexAuthHosts {
		h = strings.ToLower(h)
		if strings.Contains(host, h) {
			return true
		}
	}
	return false
}

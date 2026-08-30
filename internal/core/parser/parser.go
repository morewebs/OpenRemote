package parser

import (
	"regexp"
	"strings"
	"sync"

	"github.com/morewebs/OpenRemote/internal/core/approval"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type HitKind string

const (
	KindApproval HitKind = "approval.requested"
	KindQuestion HitKind = "question.asked"
	KindDiff     HitKind = "diff.generated"
	KindAuthURL  HitKind = "auth_url.detected"
	KindTurnDone HitKind = "turn.completed"
)

type Hit struct {
	Kind  HitKind `json:"kind"`
	Match string  `json:"match"`
}

var (
	reApprovalPrompt = regexp.MustCompile(`(?i)(?:Do you want to run|Allow\s+(?:command|tool|Bash)|Proceed with|Allow\s*[` + "`" + `"' ]?)([^` + "`" + `"'` + `\)\n\r]+)`)
	reChoiceOption   = regexp.MustCompile(`(?m)^\s*(?:❯\s*)?(\d+)[\.\)]\s*(.+)$`)
	reDiffHeader     = regexp.MustCompile(`(?m)^---\s+a\/(\S+)\s*\n\+\+\+\s+b\/(\S+)`)
	reAuthURL        = regexp.MustCompile(`https:\/\/[a-zA-Z0-9\.\-_]+\/[a-zA-Z0-9\.\-_/\?&=%+#~]+`)
	reTurnDone       = regexp.MustCompile(`(?i)(?:Done!|Completed task|Ready for next prompt|Cost: \$[\d\.]+|Tokens:)`)
)

// Scan performs a quick heuristic scan on a chunk of text.
func Scan(chunk string) []Hit {
	var hits []Hit
	if m := reApprovalPrompt.FindString(chunk); m != "" {
		hits = append(hits, Hit{Kind: KindApproval, Match: m})
	}
	if m := reDiffHeader.FindString(chunk); m != "" {
		hits = append(hits, Hit{Kind: KindDiff, Match: m})
	}
	if m := reTurnDone.FindString(chunk); m != "" {
		hits = append(hits, Hit{Kind: KindTurnDone, Match: m})
	}
	if strings.Contains(chunk, "Select an option:") || strings.Contains(chunk, "Choose an option:") {
		hits = append(hits, Hit{Kind: KindQuestion, Match: "Select an option"})
	}
	return hits
}

// StreamParser maintains state across screen lines to detect actionable agent events.
type StreamParser struct {
	mu            sync.Mutex
	sessionID     string
	seenApprovals map[string]bool
	seenQuestions map[string]bool
	recentLines   []string
	maxLines      int
}

func NewStreamParser(sessionID string) *StreamParser {
	return &StreamParser{
		sessionID:     sessionID,
		seenApprovals: make(map[string]bool),
		seenQuestions: make(map[string]bool),
		maxLines:      20,
	}
}

// FeedLine inspects a line or multi-line block and extracts events.
func (p *StreamParser) FeedLine(line string) []any {
	p.mu.Lock()
	defer p.mu.Unlock()

	p.recentLines = append(p.recentLines, line)
	if len(p.recentLines) > p.maxLines {
		p.recentLines = p.recentLines[len(p.recentLines)-p.maxLines:]
	}
	contextBlock := strings.Join(p.recentLines, "\n")

	var events []any

	// 1. Approval Detection
	if m := reApprovalPrompt.FindStringSubmatch(line); m != nil {
		rawCmd := strings.TrimSpace(m[1])
		appID := approval.GenerateID(p.sessionID, rawCmd)
		if !p.seenApprovals[appID] {
			p.seenApprovals[appID] = true
			desc := "Command execution requested"
			events = append(events, protocol.ApprovalRequestedEvent{
				BaseEvent: protocol.BaseEvent{
					SessionID: p.sessionID,
					Timestamp: protocol.NowMillis(),
				},
				Type:              protocol.EventApprovalRequested,
				ApprovalID:        appID,
				ToolName:          "Bash",
				Command:           rawCmd,
				Description:       &desc,
				AutoDenyTimeoutMs: 120000,
			})
		}
	}

	// 2. Question / Multiple choice detection
	if strings.Contains(line, "Select an option:") || strings.Contains(line, "Choose an option:") {
		matches := reChoiceOption.FindAllStringSubmatch(contextBlock, -1)
		if len(matches) >= 2 {
			var options []string
			for _, opt := range matches {
				options = append(options, opt[2])
			}
			qID := approval.GenerateID(p.sessionID, line)
			if !p.seenQuestions[qID] {
				p.seenQuestions[qID] = true
				events = append(events, protocol.QuestionAskedEvent{
					BaseEvent: protocol.BaseEvent{
						SessionID: p.sessionID,
						Timestamp: protocol.NowMillis(),
					},
					Type:          protocol.EventQuestionAsked,
					QuestionID:    qID,
					QuestionText:  line,
					Options:       options,
					IsMultiSelect: false,
				})
			}
		}
	}

	// 3. Auth URL detection
	if strings.Contains(line, "http://") || strings.Contains(line, "https://") {
		if strings.Contains(line, "login") || strings.Contains(line, "auth") || strings.Contains(line, "oauth") {
			if u := reAuthURL.FindString(line); u != "" {
				events = append(events, protocol.AuthURLEvent{
					BaseEvent: protocol.BaseEvent{
						SessionID: p.sessionID,
						Timestamp: protocol.NowMillis(),
					},
					Type: protocol.EventAuthURL,
					URL:  u,
				})
			}
		}
	}

	// 4. Diff detection
	if m := reDiffHeader.FindStringSubmatch(line); m != nil {
		events = append(events, protocol.DiffGeneratedEvent{
			BaseEvent: protocol.BaseEvent{
				SessionID: p.sessionID,
				Timestamp: protocol.NowMillis(),
			},
			Type:      protocol.EventDiffGenerated,
			FilePath:  m[1],
			DiffPatch: line,
			Additions: 0,
			Deletions: 0,
		})
	}

	return events
}

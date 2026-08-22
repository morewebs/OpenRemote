package parser

import "regexp"

// HeuristicStateMachine scans PTY output without blocking the stream — spec 02 §5.
// Go version is pure regex over buffered text; non-blocking by design.
type Kind string

const (
	KindApproval  Kind = "approval.requested"
	KindQuestion  Kind = "question.asked"
	KindDiff      Kind = "diff.generated"
	KindAuthURL   Kind = "auth_url.detected"
	KindTurnDone  Kind = "turn.completed"
)

type Hit struct {
	Kind    Kind
	Match   string
	Groups  []string
}

var (
	reApproval = regexp.MustCompile(`(?i)(?:Do you want to run|Allow)\s*[` + "`" + `"' ]?([^` + "`" + `"'` + `\)\n]+)`)
	reQuestion = regexp.MustCompile(`\?\s*Select an option:\s*\n((?:\s*\d+\)[^\n]+\n?)+)`)
	reDiff     = regexp.MustCompile(`(?m)^---\s+a\/.*?\n\+\+\+\s+b\/`)
	reAuthURL  = regexp.MustCompile(`https://claude\.ai/login\?[^\s]+`)
	reTurnDone = regexp.MustCompile(`(?i)(?:Done!|Completed task|Ready for next prompt)`)
)

func Scan(chunk string) []Hit {
	var hits []Hit
	if m := reApproval.FindStringSubmatch(chunk); m != nil {
		hits = append(hits, Hit{Kind: KindApproval, Match: m[0], Groups: m[1:]})
	}
	if m := reQuestion.FindStringSubmatch(chunk); m != nil {
		hits = append(hits, Hit{Kind: KindQuestion, Match: m[0], Groups: m[1:]})
	}
	if reDiff.MatchString(chunk) {
		hits = append(hits, Hit{Kind: KindDiff, Match: "diff"})
	}
	if m := reAuthURL.FindString(chunk); m != "" {
		hits = append(hits, Hit{Kind: KindAuthURL, Match: m})
	}
	if reTurnDone.MatchString(chunk) {
		hits = append(hits, Hit{Kind: KindTurnDone, Match: "turn.done"})
	}
	return hits
}

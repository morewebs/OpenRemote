package parser_test

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/parser"
)

func TestParserApproval(t *testing.T) {
	chunk := "Some agent text... Allow `rm -rf dist` (y/n)? Next line."
	hits := parser.Scan(chunk)

	found := false
	for _, h := range hits {
		if h.Kind == parser.KindApproval {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected approval hit in %q, got %+v", chunk, hits)
	}
}

func TestParserDiff(t *testing.T) {
	chunk := "Here is the patch:\n--- a/main.go\n+++ b/main.go\n@@ -1,3 +1,3 @@\n-old\n+new"
	hits := parser.Scan(chunk)

	found := false
	for _, h := range hits {
		if h.Kind == parser.KindDiff {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected diff hit, got %+v", hits)
	}
}

func TestParserTurnDone(t *testing.T) {
	chunk := "\nCompleted task successfully.\n"
	hits := parser.Scan(chunk)

	found := false
	for _, h := range hits {
		if h.Kind == parser.KindTurnDone {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected turn done hit, got %+v", hits)
	}
}

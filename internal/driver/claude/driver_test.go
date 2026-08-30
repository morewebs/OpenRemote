package claude

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

func TestBracketedPaste(t *testing.T) {
	got := string(BracketedPaste("line one\nline two"))
	if !strings.HasPrefix(got, "\x1b[200~") || !strings.HasSuffix(got, "\x1b[201~\r\n") {
		t.Fatalf("prompt not wrapped in bracketed paste sequences: %q", got)
	}
	if !strings.Contains(got, "line one\nline two") {
		t.Fatalf("prompt content mangled: %q", got)
	}
}

func TestBuildArgs(t *testing.T) {
	base := types.SessionConfig{SessionID: "sess1"}

	args := buildArgs(base)
	if len(args) != 1 || args[0] != "--no-auto-updater" {
		t.Fatalf("default args = %v, want [--no-auto-updater]", args)
	}

	rc := types.SessionConfig{SessionID: "sess1", RemoteControl: true, TaskName: "Fix flaky test"}
	args = buildArgs(rc)
	want := []string{"--no-auto-updater", "--remote-control", "Fix flaky test"}
	if len(args) != len(want) {
		t.Fatalf("remote-control args = %v, want %v", args, want)
	}
	for i := range want {
		if args[i] != want[i] {
			t.Fatalf("remote-control args = %v, want %v", args, want)
		}
	}

	rcNoTitle := types.SessionConfig{SessionID: "sess1", RemoteControl: true}
	args = buildArgs(rcNoTitle)
	if len(args) != 3 || args[2] != "sess1" {
		t.Fatalf("remote-control without title should fall back to sessionID: %v", args)
	}
}

func TestDetectLoginURL(t *testing.T) {
	cases := []struct {
		name string
		line string
		want string
	}{
		{
			name: "oauth authorize link",
			line: "Visit https://claude.ai/oauth/authorize?code=abc-123&state=xyz to continue",
			want: "https://claude.ai/oauth/authorize?code=abc-123&state=xyz",
		},
		{
			name: "login link with trailing punctuation",
			line: "Open https://claude.ai/login?code=abc123.",
			want: "https://claude.ai/login?code=abc123",
		},
		{
			name: "plain URL ignored",
			line: "See docs at https://example.com/login?page=1",
			want: "",
		},
		{
			name: "no url",
			line: "Do you want to run npm install?",
			want: "",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			evts := DetectLoginURL("sess_42", tc.line)
			if tc.want == "" {
				if evts != nil {
					t.Fatalf("expected no events, got %v", evts)
				}
				return
			}
			if len(evts) != 1 {
				t.Fatalf("expected 1 event, got %d", len(evts))
			}
			evt, ok := evts[0].(protocol.AuthURLEvent)
			if !ok {
				t.Fatalf("expected AuthURLEvent, got %T", evts[0])
			}
			if evt.URL != tc.want {
				t.Fatalf("URL = %q, want %q", evt.URL, tc.want)
			}
			if evt.SessionID != "sess_42" {
				t.Fatalf("SessionID = %q, want sess_42", evt.SessionID)
			}
			if evt.Type != protocol.EventAuthURL {
				t.Fatalf("Type = %q, want %q", evt.Type, protocol.EventAuthURL)
			}
		})
	}
}

func TestFindBinaryPrefersPATH(t *testing.T) {
	dir := t.TempDir()
	var name string
	if runtime.GOOS == "windows" {
		name = "claude.cmd"
	} else {
		name = "claude"
	}
	fake := filepath.Join(dir, name)
	if err := os.WriteFile(fake, []byte("#!/bin/sh\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	t.Setenv("PATH", dir+string(os.PathListSeparator)+os.Getenv("PATH"))

	d := &Driver{}
	got, err := d.findBinary()
	if err != nil {
		t.Fatalf("findBinary failed: %v", err)
	}
	if !strings.HasSuffix(got, name) {
		t.Fatalf("findBinary = %q, want path ending in %q", got, name)
	}
}

func TestDriverMetadata(t *testing.T) {
	d := &Driver{}
	if d.AgentID() != protocol.AgentClaude {
		t.Fatalf("AgentID = %v", d.AgentID())
	}
	if d.DisplayName() != "Claude Code" {
		t.Fatalf("DisplayName = %q", d.DisplayName())
	}
	caps := d.Capabilities()
	if !caps.SupportsTerminal || !caps.SupportsApproval || !caps.SupportsDiff {
		t.Fatalf("unexpected capabilities: %+v", caps)
	}
}

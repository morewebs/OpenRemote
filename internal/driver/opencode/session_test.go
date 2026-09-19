package opencode

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type testSink struct {
	messages []chat.Message
	events   []any
}

func (*testSink) Bytes([]byte)               {}
func (s *testSink) Message(msg chat.Message) { s.messages = append(s.messages, msg) }
func (s *testSink) Event(evt any)            { s.events = append(s.events, evt) }
func (*testSink) Exit(int, string)           {}

func TestHTTPBridgeApprovalsQuestionsAndPrompt(t *testing.T) {
	var paths []string
	var bodies []map[string]any
	api := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		user, password, ok := r.BasicAuth()
		if !ok || user != "openremote" || password != "secret" {
			t.Error("missing per-process authentication")
		}
		if r.Header.Get("x-opencode-directory") != "workspace" {
			t.Error("missing directory scope")
		}
		paths = append(paths, r.URL.Path)
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		bodies = append(bodies, body)
		w.WriteHeader(http.StatusNoContent)
	}))
	defer api.Close()
	sink := &testSink{}
	s := newHTTPSession(context.Background(), types.SessionConfig{SessionID: "local", CWD: "workspace"}, sink)
	defer s.Close()
	s.baseURL, s.password, s.sessionID = api.URL, "secret", "remote"
	if err := s.Prompt("hello"); err != nil {
		t.Fatal(err)
	}
	s.handleEvent([]byte(`{"type":"permission.asked","properties":{"id":"p1","sessionID":"remote","permission":"bash","patterns":["go test ./..."]}}`))
	approval := sink.events[0].(protocol.ApprovalRequestedEvent)
	if err := s.Approve(approval.ApprovalID, false); err != nil {
		t.Fatal(err)
	}
	if bodies[1]["reply"] != "reject" {
		t.Fatalf("denial became %v", bodies[1])
	}
	s.handleEvent([]byte(`{"type":"question.asked","properties":{"id":"q1","sessionID":"remote","questions":[{"question":"Color?","options":[{"label":"Red"}]},{"question":"Size?","options":[{"label":"Small"}]}]}}`))
	first := sink.events[1].(protocol.QuestionAskedEvent)
	second := sink.events[2].(protocol.QuestionAskedEvent)
	if err := s.Answer(first.QuestionID, "Red"); err != nil {
		t.Fatal(err)
	}
	if len(paths) != 2 {
		t.Fatal("sent incomplete answer group")
	}
	if err := s.Answer(second.QuestionID, "Small"); err != nil {
		t.Fatal(err)
	}
	if paths[0] != "/session/remote/prompt_async" || paths[1] != "/permission/p1/reply" || paths[2] != "/question/q1/reply" {
		t.Fatalf("routes = %v", paths)
	}
	answers := bodies[2]["answers"].([]any)
	if len(answers) != 2 || answers[0].([]any)[0] != "Red" || answers[1].([]any)[0] != "Small" {
		t.Fatalf("answers = %v", answers)
	}
}

func TestSSEFragmentationAndSessionIsolation(t *testing.T) {
	sink := &testSink{}
	s := newHTTPSession(context.Background(), types.SessionConfig{SessionID: "local"}, sink)
	defer s.Close()
	s.sessionID = "remote"
	events := ": keepalive\n\ndata: {\"type\":\"message.updated\",\n" +
		"data: \"properties\":{\"info\":{\"id\":\"m\",\"sessionID\":\"remote\",\"role\":\"assistant\"}}}\n\n" +
		"data: {\"type\":\"message.part.updated\",\"properties\":{\"part\":{\"id\":\"p\",\"messageID\":\"m\",\"sessionID\":\"remote\",\"type\":\"text\",\"text\":\"Hello\"}}}\n\n" +
		"data: {\"type\":\"message.part.delta\",\"properties\":{\"partID\":\"p\",\"messageID\":\"m\",\"sessionID\":\"remote\",\"field\":\"text\",\"delta\":\" world\"}}\n\n" +
		"data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"another\"}}\n\n"
	if err := consumeSSE(strings.NewReader(events), s.handleEvent); err != nil {
		t.Fatal(err)
	}
	if len(sink.messages) != 2 || sink.messages[1].Text != "Hello world" || len(sink.events) != 0 {
		t.Fatalf("messages=%+v events=%v", sink.messages, sink.events)
	}
}

func TestContentDiffEventsProduceReviewablePatch(t *testing.T) {
	sink := &testSink{}
	s := newHTTPSession(context.Background(), types.SessionConfig{SessionID: "local"}, sink)
	defer s.Close()
	s.sessionID = "remote"
	s.handleEvent([]byte(`{"type":"session.diff","properties":{"sessionID":"remote","diff":[{"file":"hello.txt","before":"old\n","after":"new\n","additions":1,"deletions":1}]}}`))
	if len(sink.events) != 1 {
		t.Fatalf("events = %v", sink.events)
	}
	diff := sink.events[0].(protocol.DiffGeneratedEvent)
	if !strings.Contains(diff.DiffPatch, "-old\n+new\n") || !strings.Contains(diff.DiffPatch, "@@ -1,1 +1,1 @@") {
		t.Fatalf("missing content diff: %q", diff.DiffPatch)
	}
}

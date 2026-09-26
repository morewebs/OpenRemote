package pi

import (
	"encoding/json"
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/transport"
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

func TestRPCResponsesAndMessageStream(t *testing.T) {
	response, err := decodeRPC([]byte(`{"id":"r1","type":"response","command":"prompt","success":false,"error":"No model configured"}`))
	if err != nil || response.Error == nil || response.Error.Message != "No model configured" || string(response.ID) != `"r1"` {
		t.Fatalf("response=%+v err=%v", response, err)
	}
	sink := &testSink{}
	s := &rpcSession{cfg: types.SessionConfig{SessionID: "s"}, sink: sink, blocks: make(map[int]string), requests: make(map[string]string)}
	feed := func(kind, body string) { s.handle(transport.Message{Method: kind, Params: json.RawMessage(body)}) }
	feed("message_start", `{"message":{"role":"assistant"}}`)
	feed("message_update", `{"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"hello"}}`)
	feed("message_end", `{"message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}`)
	if len(sink.messages) != 2 || sink.messages[1].Streaming || sink.messages[1].Text != "hello" || sink.messages[1].Rev <= sink.messages[0].Rev {
		t.Fatalf("messages=%+v", sink.messages)
	}
	feed("extension_ui_request", `{"id":"q1","method":"select","title":"Which branch?","options":["main","dev"]}`)
	question := sink.events[0].(protocol.QuestionAskedEvent)
	if s.requests[question.QuestionID] != "q1" || len(question.Options) != 2 {
		t.Fatalf("question=%+v", question)
	}
	feed("extension_ui_request", `{"id":"a1","method":"confirm","title":"Deploy?","message":"Confirm deployment","timeout":30000}`)
	approval := sink.events[1].(protocol.ApprovalRequestedEvent)
	if approval.Command != "Deploy?" || approval.AutoDenyTimeoutMs != 30000 {
		t.Fatalf("approval=%+v", approval)
	}
}

// The pi preamble arrives as a system message whose text field is empty because
// the content lives in structured sections. Emitting it produced a blank card.
func TestSystemPreambleDoesNotEmitACard(t *testing.T) {
	sink := &testSink{}
	s := &rpcSession{cfg: types.SessionConfig{SessionID: "s"}, sink: sink, blocks: make(map[int]string), requests: make(map[string]string)}
	feed := func(kind, body string) { s.handle(transport.Message{Method: kind, Params: json.RawMessage(body)}) }
	feed("message_start", `{"message":{"role":"system"}}`)
	feed("message_end", `{"message":{"role":"system","content":"","sections":{"preamble":"You are an expert coding assistant."}}}`)
	if len(sink.messages) != 0 {
		t.Fatalf("system preamble emitted %+v", sink.messages)
	}
	// The user prompt is recorded by the shared handler, so the driver echo is
	// still forwarded (the server drops it later).
	feed("message_start", `{"message":{"role":"user"}}`)
	feed("message_end", `{"message":{"role":"user","content":[{"type":"text","text":"hi"}]}}`)
	if len(sink.messages) != 1 || sink.messages[0].Role != protocol.RoleUser || sink.messages[0].Text != "hi" {
		t.Fatalf("messages=%+v", sink.messages)
	}
}

// A provider failure ends the assistant message with stopReason "error" and no
// content, so the reason must become visible.
func TestAssistantErrorReasonIsSurfaced(t *testing.T) {
	sink := &testSink{}
	s := &rpcSession{cfg: types.SessionConfig{SessionID: "s"}, sink: sink, blocks: make(map[int]string), requests: make(map[string]string)}
	feed := func(kind, body string) { s.handle(transport.Message{Method: kind, Params: json.RawMessage(body)}) }
	feed("message_start", `{"message":{"role":"assistant","content":[]}}`)
	feed("message_end", `{"message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"403: Access denied"}}`)
	if len(sink.messages) != 1 || sink.messages[0].Kind != "error" || sink.messages[0].Text != "403: Access denied" {
		t.Fatalf("messages=%+v", sink.messages)
	}

	// Partial text survives and the reason is appended rather than replacing it.
	sink2 := &testSink{}
	s2 := &rpcSession{cfg: types.SessionConfig{SessionID: "s"}, sink: sink2, blocks: make(map[int]string), requests: make(map[string]string)}
	feed2 := func(kind, body string) { s2.handle(transport.Message{Method: kind, Params: json.RawMessage(body)}) }
	feed2("message_start", `{"message":{"role":"assistant","content":[]}}`)
	feed2("message_update", `{"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"partial"}}`)
	feed2("message_end", `{"message":{"role":"assistant","content":[{"type":"text","text":"partial"}],"stopReason":"error","errorMessage":"socket closed"}}`)
	if len(sink2.messages) == 0 {
		t.Fatal("no messages")
	}
	last := sink2.messages[len(sink2.messages)-1]
	if last.Kind != "error" || last.Text != "partial\n\nsocket closed" {
		t.Fatalf("last=%+v", last)
	}
}

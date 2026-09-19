package antigravity

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type watcherTestSink struct {
	events   chan any
	messages chan chat.Message
}

func (*watcherTestSink) Bytes([]byte)               {}
func (s *watcherTestSink) Message(msg chat.Message) { s.messages <- msg }
func (s *watcherTestSink) Event(evt any)            { s.events <- evt }
func (*watcherTestSink) Exit(int, string)           {}

func TestArtifactAndPartialTranscript(t *testing.T) {
	root := t.TempDir()
	sink := &watcherTestSink{events: make(chan any, 10), messages: make(chan chat.Message, 10)}
	w, err := startWatcher(context.Background(), types.SessionConfig{SessionID: "s", CWD: root}, sink)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close()
	if err := os.WriteFile(filepath.Join(root, "implementation_plan.md"), []byte("# Plan\nRun tests"), 0600); err != nil {
		t.Fatal(err)
	}
	select {
	case evt := <-sink.events:
		if artifact, ok := evt.(protocol.ArtifactUpdatedEvent); !ok || artifact.Kind != "plan" || artifact.Path != "implementation_plan.md" {
			t.Fatalf("artifact=%+v", evt)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("artifact not observed")
	}
	file, err := os.OpenFile(filepath.Join(root, "transcript.jsonl"), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0600)
	if err != nil {
		t.Fatal(err)
	}
	_, _ = file.WriteString(`{"type":"invoke_subagent","arguments":{"name":"reviewer",`)
	_ = file.Sync()
	select {
	case <-sink.messages:
		t.Fatal("partial JSON emitted a message")
	case <-time.After(350 * time.Millisecond):
	}
	_, _ = file.WriteString("\"task\":\"Review changes\"}}\n")
	_ = file.Close()
	select {
	case msg := <-sink.messages:
		if msg.Kind != "subagent" || msg.Text != "reviewer: started\nReview changes" {
			t.Fatalf("message=%+v", msg)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("transcript not observed")
	}
}

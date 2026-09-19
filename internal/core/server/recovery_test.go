package server_test

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

func recoveryServer(t *testing.T) (*server.Server, *httptest.Server, string) {
	t.Helper()
	root := t.TempDir()
	bus, err := events.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = bus.Close() })
	srv := newTestServer(t, server.Config{DataDir: root, AllowedRoots: []string{root}}, bus)
	ts := httptest.NewServer(srv.Handler())
	t.Cleanup(ts.Close)
	return srv, ts, root
}

func TestReplayPreservesSplitUTF8AndSSECursor(t *testing.T) {
	srv, ts, _ := recoveryServer(t)
	srv.TestInjectSession("replay", &mockDriverSession{})
	sink := srv.TestSink("replay")
	raw := []byte("hello 🌍\n")
	sink.Bytes(raw[:8]) // Split inside the four-byte codepoint.
	sink.Bytes(raw[8:])
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	conn, _, err := websocket.Dial(ctx, "ws"+ts.URL[4:]+"/ws?sessionId=replay&eventsOnly=1&lastSeq=0", nil)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.CloseNow()
	var replay []byte
	var cursor int64
	for i := 0; i < 2; i++ {
		event := readJSONRPCEvent(t, ctx, conn)
		if event["type"] != "stream.chunk" {
			t.Fatalf("unexpected replay: %v", event)
		}
		seq := int64(event["seq"].(float64))
		if seq <= cursor {
			t.Fatal("nonmonotonic replay")
		}
		cursor = seq
		chunk, err := base64.StdEncoding.DecodeString(event["chunk"].(string))
		if err != nil {
			t.Fatal(err)
		}
		replay = append(replay, chunk...)
	}
	if !bytes.Equal(replay, raw) {
		t.Fatalf("replay bytes %q", replay)
	}
	// Last-Event-ID takes precedence over the query cursor, then live delivery
	// continues from the same durable stream without replaying the old chunks.
	req, _ := http.NewRequestWithContext(ctx, http.MethodGet, ts.URL+"/events?sessionId=replay&lastSeq=0", nil)
	req.Header.Set("Last-Event-ID", strconv.FormatInt(cursor, 10))
	response, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	sink.Bytes([]byte("live\n"))
	scan := bufio.NewScanner(response.Body)
	for scan.Scan() {
		if !strings.HasPrefix(scan.Text(), "data: ") {
			continue
		}
		var event map[string]any
		if err := json.Unmarshal([]byte(strings.TrimPrefix(scan.Text(), "data: ")), &event); err != nil {
			t.Fatal(err)
		}
		chunk, _ := base64.StdEncoding.DecodeString(event["chunk"].(string))
		if string(chunk) != "live\n" || int64(event["seq"].(float64)) <= cursor {
			t.Fatalf("bad live event: %v", event)
		}
		return
	}
	t.Fatalf("live stream ended: %v", scan.Err())
}

func TestFilesContainmentAndBinaryPreview(t *testing.T) {
	_, ts, root := recoveryServer(t)
	inside := filepath.Join(root, "notes.txt")
	if err := os.WriteFile(inside, []byte("safe"), 0600); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(t.TempDir(), "private.txt")
	if err := os.WriteFile(outside, []byte("private"), 0600); err != nil {
		t.Fatal(err)
	}
	binary := filepath.Join(root, "binary")
	if err := os.WriteFile(binary, []byte{0, 1, 2}, 0600); err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		path   string
		status int
	}{{inside, 200}, {outside, 403}, {binary, 415}, {filepath.Join(root, "..", "private.txt"), 403}}
	link := filepath.Join(root, "escape.txt")
	if err := os.Symlink(outside, link); err == nil {
		cases = append(cases, struct {
			path   string
			status int
		}{link, 403})
	} else {
		t.Logf("symlink creation unavailable: %v", err)
	}
	for _, tc := range cases {
		resp, err := http.Get(ts.URL + "/api/v1/file?path=" + url.QueryEscape(tc.path))
		if err != nil {
			t.Fatal(err)
		}
		body, _ := io.ReadAll(resp.Body)
		resp.Body.Close()
		if resp.StatusCode != tc.status {
			t.Errorf("%s: status %d, body %s", tc.path, resp.StatusCode, body)
		}
		if tc.status == 403 && bytes.Contains(body, []byte("private\"")) {
			t.Fatal("outside file leaked")
		}
	}
}

type blockingAnswerSession struct {
	mockDriverSession
	entered chan struct{}
	release chan struct{}
	once    sync.Once
}

func (s *blockingAnswerSession) Answer(id string, value any) error {
	s.once.Do(func() { close(s.entered) })
	<-s.release
	return s.mockDriverSession.Answer(id, value)
}

func TestQuestionClaimPreventsConcurrentDelivery(t *testing.T) {
	srv, ts, _ := recoveryServer(t)
	drv := &blockingAnswerSession{entered: make(chan struct{}), release: make(chan struct{})}
	srv.TestInjectSession("questions", drv)
	srv.TestSink("questions").Event(protocol.QuestionAskedEvent{BaseEvent: protocol.BaseEvent{SessionID: "questions"}, Type: protocol.EventQuestionAsked, QuestionID: "q1", QuestionText: "Choose", Options: []string{"yes"}})
	answer := func() int {
		resp, err := http.Post(ts.URL+"/api/v1/question/q1", "application/json", strings.NewReader(`{"answers":["yes"]}`))
		if err != nil {
			return 0
		}
		defer resp.Body.Close()
		return resp.StatusCode
	}
	first := make(chan int, 1)
	go func() { first <- answer() }()
	select {
	case <-drv.entered:
	case <-time.After(3 * time.Second):
		close(drv.release)
		t.Fatal("answer did not arrive")
	}
	second := answer()
	close(drv.release)
	if second != 404 || <-first != 200 {
		t.Fatalf("concurrent answer status=%d", second)
	}
	drv.mu.Lock()
	defer drv.mu.Unlock()
	if len(drv.answers) != 1 {
		t.Fatalf("delivered %d times", len(drv.answers))
	}
}

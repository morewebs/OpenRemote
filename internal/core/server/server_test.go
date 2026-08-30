package server_test

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"

	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type mockAnswer struct {
	questionID string
	answer     any
}

type mockDriverSession struct {
	mu      sync.Mutex
	answers []mockAnswer
}

func (m *mockDriverSession) Prompt(text string) error             { return nil }
func (m *mockDriverSession) RawInput(data []byte) error           { return nil }
func (m *mockDriverSession) Resize(cols, rows int) error          { return nil }
func (m *mockDriverSession) Snapshot() []byte                     { return nil }
func (m *mockDriverSession) Approve(id string, ok bool) error     { return nil }
func (m *mockDriverSession) Close() error                         { return nil }

func (m *mockDriverSession) Answer(questionID string, answer any) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.answers = append(m.answers, mockAnswer{questionID: questionID, answer: answer})
	return nil
}

func (m *mockDriverSession) waitForAnswer(t *testing.T, questionID string) mockAnswer {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		m.mu.Lock()
		for _, a := range m.answers {
			if a.questionID == questionID {
				m.mu.Unlock()
				return a
			}
		}
		m.mu.Unlock()
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("driver never received answer for question %s", questionID)
	return mockAnswer{}
}

// readJSONRPCEvent reads frames until a JSONRPC frame arrives, skipping
// PTY output echoes, and returns the decoded event payload.
func readJSONRPCEvent(t *testing.T, ctx context.Context, conn *websocket.Conn) map[string]any {
	t.Helper()
	for {
		_, data, err := conn.Read(ctx)
		if err != nil {
			t.Fatalf("ws read failed: %v", err)
		}
		frame, err := protocol.Decode(data)
		if err != nil {
			t.Fatalf("frame decode failed: %v", err)
		}
		if frame.Opcode != protocol.OpcodeJSONRPC {
			continue
		}
		var evt map[string]any
		if err := json.Unmarshal(frame.Payload, &evt); err != nil {
			t.Fatalf("unmarshal event failed: %v", err)
		}
		return evt
	}
}

func fetchEventsSince(t *testing.T, base, token, sessionID string, since int64) []map[string]any {
	t.Helper()
	req, _ := http.NewRequest(http.MethodGet, fmt.Sprintf("%s/api/v1/sessions/%s?since=%d", base, sessionID, since), nil)
	req.Header.Set("Authorization", "Bearer "+token)
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("events request failed: %v", err)
	}
	defer res.Body.Close()
	if res.StatusCode != 200 {
		t.Fatalf("expected 200 on events catchup, got %d", res.StatusCode)
	}
	var evs []map[string]any
	if err := json.NewDecoder(res.Body).Decode(&evs); err != nil {
		t.Fatalf("decode events failed: %v", err)
	}
	return evs
}

func TestServerHealthAndSessions(t *testing.T) {
	tempDir := t.TempDir()
	bus, err := events.Open(tempDir)
	if err != nil {
		t.Fatal(err)
	}
	defer bus.Close()

	cwd, _ := os.Getwd()

	srv := server.New(server.Config{
		Addr:         "127.0.0.1:0",
		DataDir:      tempDir,
		Token:        "test-token",
		AllowedRoots: []string{cwd, tempDir},
	}, bus)

	ts := httptest.NewServer(srv.Handler())
	defer ts.Close()

	// 1. Check health without auth
	res, err := http.Get(ts.URL + "/health")
	if err != nil {
		t.Fatalf("health probe failed: %v", err)
	}
	defer res.Body.Close()
	if res.StatusCode != 200 {
		t.Errorf("health status code = %d", res.StatusCode)
	}

	var health protocol.HealthResponse
	if err := json.NewDecoder(res.Body).Decode(&health); err != nil {
		t.Fatalf("decode health failed: %v", err)
	}
	if health.Status != "ok" {
		t.Errorf("health status = %s", health.Status)
	}

	// 2. Create session with auth (use shell for universal test)
	createReq := protocol.CreateSessionRequest{
		AgentID:     protocol.AgentShell,
		CWD:         cwd,
		UseWorktree: false,
	}
	reqBytes, _ := json.Marshal(createReq)

	req, _ := http.NewRequest(http.MethodPost, ts.URL+"/api/v1/sessions", bytes.NewReader(reqBytes))
	req.Header.Set("Authorization", "Bearer test-token")
	req.Header.Set("Content-Type", "application/json")

	createRes, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("create session request failed: %v", err)
	}
	defer createRes.Body.Close()

	if createRes.StatusCode != 201 {
		t.Fatalf("expected 201 Created, got %d", createRes.StatusCode)
	}

	var createResp protocol.CreateSessionResponse
	if err := json.NewDecoder(createRes.Body).Decode(&createResp); err != nil {
		t.Fatalf("decode create response failed: %v", err)
	}
	if createResp.SessionID == "" {
		t.Error("expected non-empty SessionID")
	}

	// 3. Connect via WebSocket to the new session (using ?token query param)
	wsURL := "ws" + ts.URL[4:] + "/ws?sessionId=" + createResp.SessionID + "&token=test-token"
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	wsConn, _, err := websocket.Dial(ctx, wsURL, nil)
	if err != nil {
		t.Fatalf("websocket connection failed: %v", err)
	}
	defer wsConn.Close(websocket.StatusNormalClosure, "")

	// Send keystroke frame
	keyFrame := protocol.Encode(protocol.OpcodeKeystroke, 0, []byte("echo test\n"))
	if err := wsConn.Write(ctx, websocket.MessageBinary, keyFrame); err != nil {
		t.Fatalf("ws write failed: %v", err)
	}
}

func TestServer_QuestionLifecycleAndAnswerRouting(t *testing.T) {
	tempDir := t.TempDir()
	bus, err := events.Open(tempDir)
	if err != nil {
		t.Fatal(err)
	}
	defer bus.Close()

	cwd, _ := os.Getwd()

	srv := server.New(server.Config{
		Addr:         "127.0.0.1:0",
		DataDir:      tempDir,
		Token:        "test-token",
		AllowedRoots: []string{cwd, tempDir},
	}, bus)

	ts := httptest.NewServer(srv.Handler())
	defer ts.Close()

	mock := &mockDriverSession{}
	sessionID := "sess-question"
	srv.TestInjectSession(sessionID, mock)

	wsURL := "ws" + ts.URL[4:] + "/ws?sessionId=" + sessionID + "&token=test-token"
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	wsConn, _, err := websocket.Dial(ctx, wsURL, nil)
	if err != nil {
		t.Fatalf("websocket connection failed: %v", err)
	}
	defer wsConn.Close(websocket.StatusNormalClosure, "")

	// Drive the parser via the same sink a real driver uses. The block
	// triggers question detection ("Select an option:" + 2 numbered options).
	sink := srv.TestSink(sessionID)
	sink.Bytes([]byte("Select an option:\n1. Red\n2. Blue\n"))

	// The question.asked event must arrive over the live WS connection.
	askedEvt := readJSONRPCEvent(t, ctx, wsConn)
	if askedEvt["type"] != string(protocol.EventQuestionAsked) {
		t.Fatalf("expected question.asked event, got %v", askedEvt["type"])
	}
	questionID, _ := askedEvt["questionId"].(string)
	if questionID == "" {
		t.Fatal("expected non-empty questionId in question.asked event")
	}

	// Post an answer through the REST endpoint.
	replyBody, _ := json.Marshal(protocol.QuestionReply{Answers: []any{"Red"}})
	req, _ := http.NewRequest(http.MethodPost, ts.URL+"/api/v1/question/"+questionID, bytes.NewReader(replyBody))
	req.Header.Set("Authorization", "Bearer test-token")
	req.Header.Set("Content-Type", "application/json")

	res, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("question answer request failed: %v", err)
	}
	defer res.Body.Close()
	if res.StatusCode != 200 {
		t.Fatalf("expected 200 on question answer, got %d", res.StatusCode)
	}
	var okResp map[string]any
	if err := json.NewDecoder(res.Body).Decode(&okResp); err != nil {
		t.Fatalf("decode answer response failed: %v", err)
	}
	if okResp["ok"] != true {
		t.Errorf("expected {\"ok\":true}, got %v", okResp)
	}

	// The scalar payload must reach the driver session.
	got := mock.waitForAnswer(t, questionID)
	if got.answer != "Red" {
		t.Errorf("expected driver to receive scalar answer \"Red\", got %#v", got.answer)
	}

	// QuestionAnsweredEvent must be broadcast over the live WS connection.
	answeredEvt := readJSONRPCEvent(t, ctx, wsConn)
	if answeredEvt["type"] != string(protocol.EventQuestionAnswered) {
		t.Fatalf("expected question.answered event, got %v", answeredEvt["type"])
	}
	if answeredEvt["questionId"] != questionID {
		t.Errorf("questionId mismatch: got %v, want %s", answeredEvt["questionId"], questionID)
	}
	answers, _ := answeredEvt["answers"].([]any)
	if len(answers) != 1 || answers[0] != "Red" {
		t.Errorf("expected answers [\"Red\"], got %v", answers)
	}

	// Both events must be persisted for catchup.
	evs := fetchEventsSince(t, ts.URL, "test-token", sessionID, 0)
	var sawAsked, sawAnswered bool
	var lastSeq int64
	for _, ev := range evs {
		seq := int64(ev["seq"].(float64))
		if seq <= lastSeq {
			t.Errorf("event seq not ascending: %d after %d", seq, lastSeq)
		}
		lastSeq = seq
		switch ev["type"] {
		case string(protocol.EventQuestionAsked):
			sawAsked = true
		case string(protocol.EventQuestionAnswered):
			sawAnswered = true
		}
	}
	if !sawAsked || !sawAnswered {
		t.Errorf("expected persisted question.asked and question.answered, got asked=%v answered=%v", sawAsked, sawAnswered)
	}
}

func TestServer_EventCatchupHydration(t *testing.T) {
	tempDir := t.TempDir()
	bus, err := events.Open(tempDir)
	if err != nil {
		t.Fatal(err)
	}
	defer bus.Close()

	cwd, _ := os.Getwd()

	srv := server.New(server.Config{
		Addr:         "127.0.0.1:0",
		DataDir:      tempDir,
		Token:        "test-token",
		AllowedRoots: []string{cwd, tempDir},
	}, bus)

	ts := httptest.NewServer(srv.Handler())
	defer ts.Close()

	sessionID := "sess-hydration"
	srv.TestInjectSession(sessionID, &mockDriverSession{})

	// Seed five sequential events directly on the bus.
	seqs := make([]int64, 5)
	for i := 0; i < 5; i++ {
		evt := protocol.StreamChunkEvent{
			BaseEvent: protocol.BaseEvent{
				SessionID: sessionID,
				Timestamp: protocol.NowMillis(),
			},
			Type:  protocol.EventStreamChunk,
			Chunk: fmt.Sprintf("chunk-%d", i),
		}
		seq, err := bus.AppendEvent(sessionID, string(protocol.EventStreamChunk), evt)
		if err != nil {
			t.Fatalf("AppendEvent failed: %v", err)
		}
		seqs[i] = seq
	}

	// Catchup from the second event's seq must return only strictly newer events.
	evs := fetchEventsSince(t, ts.URL, "test-token", sessionID, seqs[1])
	if len(evs) != 3 {
		t.Fatalf("expected 3 events since seq %d, got %d", seqs[1], len(evs))
	}
	prev := seqs[1]
	for i, ev := range evs {
		seq := int64(ev["seq"].(float64))
		if seq <= prev {
			t.Errorf("event %d: seq %d not strictly greater than %d", i, seq, prev)
		}
		prev = seq
		wantChunk := fmt.Sprintf("chunk-%d", i+2)
		if ev["chunk"] != wantChunk {
			t.Errorf("event %d: expected chunk %q, got %v", i, wantChunk, ev["chunk"])
		}
	}

	// Catchup from zero must return the full history.
	all := fetchEventsSince(t, ts.URL, "test-token", sessionID, 0)
	if len(all) != 5 {
		t.Fatalf("expected 5 events from seq 0, got %d", len(all))
	}
}

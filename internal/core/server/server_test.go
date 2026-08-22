package server_test

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"

	"github.com/coder/websocket"

	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

func TestServerHealthAndSessions(t *testing.T) {
	tempDir := t.TempDir()
	bus, err := events.Open(tempDir)
	if err != nil {
		t.Fatal(err)
	}
	defer bus.Close()

	srv := server.New(server.Config{
		Addr:    "127.0.0.1:0",
		DataDir: tempDir,
		Token:   "test-token",
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

	// 2. Create session with auth
	cwd, _ := os.Getwd()
	createReq := protocol.CreateSessionRequest{
		AgentID:     protocol.AgentClaude,
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

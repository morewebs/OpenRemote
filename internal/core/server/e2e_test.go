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

func TestEndToEndSessionLifecycle(t *testing.T) {
	tempDir := t.TempDir()
	bus, err := events.Open(tempDir)
	if err != nil {
		t.Fatalf("events.Open failed: %v", err)
	}
	defer bus.Close()

	cwd, _ := os.Getwd()
	token := "secret-test-token-1234567890"

	srv := server.New(server.Config{
		Addr:         "127.0.0.1:0",
		DataDir:      tempDir,
		Token:        token,
		AllowedRoots: []string{cwd, tempDir},
	}, bus)

	ts := httptest.NewServer(srv.Handler())
	defer ts.Close()

	// 1. Health check
	healthRes, err := http.Get(ts.URL + "/health")
	if err != nil {
		t.Fatalf("health probe failed: %v", err)
	}
	defer healthRes.Body.Close()
	if healthRes.StatusCode != 200 {
		t.Fatalf("expected 200 health, got %d", healthRes.StatusCode)
	}

	// 2. Agents list
	req, _ := http.NewRequest(http.MethodGet, ts.URL+"/api/v1/agents", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	agentsRes, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("agents list request failed: %v", err)
	}
	defer agentsRes.Body.Close()
	if agentsRes.StatusCode != 200 {
		t.Fatalf("expected 200 agents, got %d", agentsRes.StatusCode)
	}

	var agents []protocol.AgentInfo
	_ = json.NewDecoder(agentsRes.Body).Decode(&agents)
	if len(agents) == 0 {
		t.Fatalf("expected registered agents, got 0")
	}

	// 3. Create Session with Shell agent
	createBody, _ := json.Marshal(protocol.CreateSessionRequest{
		AgentID:     protocol.AgentShell,
		CWD:         cwd,
		UseWorktree: false,
		Cols:        100,
		Rows:        30,
	})
	createReq, _ := http.NewRequest(http.MethodPost, ts.URL+"/api/v1/sessions", bytes.NewReader(createBody))
	createReq.Header.Set("Authorization", "Bearer "+token)
	createReq.Header.Set("Content-Type", "application/json")

	createRes, err := http.DefaultClient.Do(createReq)
	if err != nil {
		t.Fatalf("create session failed: %v", err)
	}
	defer createRes.Body.Close()

	if createRes.StatusCode != 201 {
		t.Fatalf("expected 201 Created, got %d", createRes.StatusCode)
	}

	var createResp protocol.CreateSessionResponse
	_ = json.NewDecoder(createRes.Body).Decode(&createResp)
	sessionID := createResp.SessionID
	if sessionID == "" {
		t.Fatalf("expected valid sessionID")
	}

	// 4. WebSocket connect and type command
	wsURL := "ws" + ts.URL[4:] + "/ws?sessionId=" + sessionID + "&token=" + token
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	wsConn, _, err := websocket.Dial(ctx, wsURL, nil)
	if err != nil {
		t.Fatalf("ws dial failed: %v", err)
	}
	defer wsConn.Close(websocket.StatusNormalClosure, "")

	// Send echo keystroke
	keyFrame := protocol.Encode(protocol.OpcodeKeystroke, 0, []byte("echo hello_openremote\r\n"))
	if err := wsConn.Write(ctx, websocket.MessageBinary, keyFrame); err != nil {
		t.Fatalf("ws write failed: %v", err)
	}

	// 5. Send Prompt via REST
	promptBody, _ := json.Marshal(map[string]string{"prompt": "echo prompt_test"})
	promptReq, _ := http.NewRequest(http.MethodPost, ts.URL+"/api/v1/sessions/"+sessionID+"/prompt", bytes.NewReader(promptBody))
	promptReq.Header.Set("Authorization", "Bearer "+token)
	promptReq.Header.Set("Content-Type", "application/json")

	promptRes, err := http.DefaultClient.Do(promptReq)
	if err != nil {
		t.Fatalf("prompt request failed: %v", err)
	}
	defer promptRes.Body.Close()
	if promptRes.StatusCode != 200 {
		t.Fatalf("expected 200 prompt, got %d", promptRes.StatusCode)
	}

	// 6. Delete Session
	delReq, _ := http.NewRequest(http.MethodDelete, ts.URL+"/api/v1/sessions/"+sessionID, nil)
	delReq.Header.Set("Authorization", "Bearer "+token)
	delRes, err := http.DefaultClient.Do(delReq)
	if err != nil {
		t.Fatalf("delete request failed: %v", err)
	}
	defer delRes.Body.Close()
	if delRes.StatusCode != 204 && delRes.StatusCode != 200 {
		t.Fatalf("expected 204 or 200 on delete, got %d", delRes.StatusCode)
	}
}

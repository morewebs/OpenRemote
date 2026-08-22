package server

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/coder/websocket"

	"github.com/morewebs/OpenRemote/internal/core/auth"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/parser"
	"github.com/morewebs/OpenRemote/internal/core/workspace"
	"github.com/morewebs/OpenRemote/internal/driver"
	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
)

type Config struct {
	Addr    string // 127.0.0.1:4097 default
	DataDir string // ~/.openremote/data
	Token   string // empty = no auth (dev)
}

type Server struct {
	cfg        Config
	bus        *events.Bus
	ptyManager *pty.Manager
	drivers    *driver.Registry
	http       *http.Server
	mu         sync.Mutex
	startTime  time.Time
	sessions   map[string]*SessionState
}

type SessionState struct {
	SessionID    string
	WorkspaceID  string
	AgentID      protocol.AgentID
	CWD          string
	WorktreePath string
	BranchName   string
	Status       protocol.SessionStatus
	CreatedAt    int64
	Hub          *Hub // WS fan-out
}

type Hub struct {
	mu      sync.RWMutex
	clients map[*wsClient]struct{}
}

type wsClient struct {
	slot int
	send chan []byte
}

func NewHub() *Hub { return &Hub{clients: make(map[*wsClient]struct{})} }
func (h *Hub) Add(c *wsClient) { h.mu.Lock(); h.clients[c] = struct{}{}; h.mu.Unlock() }
func (h *Hub) Remove(c *wsClient) { h.mu.Lock(); delete(h.clients, c); h.mu.Unlock() }
func (h *Hub) Broadcast(frame []byte) {
	h.mu.RLock()
	for c := range h.clients {
		select { case c.send <- frame: default: }
	}
	h.mu.RUnlock()
}

func New(cfg Config, bus *events.Bus) *Server {
	if cfg.Addr == "" {
		cfg.Addr = "127.0.0.1:4097"
	}
	return &Server{
		cfg:        cfg,
		bus:        bus,
		ptyManager: pty.NewManager(),
		drivers:    driver.NewRegistry(nil),
		sessions:   make(map[string]*SessionState),
		startTime:  time.Now(),
	}
}

func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", s.handleHealth)
	mux.HandleFunc("/ws", s.handleWS)
	mux.HandleFunc("/events", s.handleSSE)
	mux.HandleFunc("/api/v1/sessions", s.handleSessions)
	mux.HandleFunc("/api/v1/sessions/", s.handleSessionByID)
	mux.HandleFunc("/api/v1/approval/", s.handleApproval)
	mux.HandleFunc("/api/v1/question/", s.handleQuestion)
	mux.HandleFunc("/api/v1/files", s.handleFiles)
	mux.HandleFunc("/api/v1/diff/", s.handleDiff)

	var h http.Handler = mux
	h = auth.Middleware(s.cfg.Token, h)
	// CORS for web-pwa dev
	h = corsMiddleware(h)
	return h
}

func corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Authorization, Content-Type")
		w.Header().Set("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(204)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func (s *Server) ListenAndServe() error {
	s.http = &http.Server{Addr: s.cfg.Addr, Handler: s.Handler(), ReadHeaderTimeout: 5 * time.Second}
	log.Printf("[core] listening on http://%s  data=%s", s.cfg.Addr, s.cfg.DataDir)
	return s.http.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	if s.http != nil {
		return s.http.Shutdown(ctx)
	}
	return nil
}

// --- health ---

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	s.mu.Lock()
	n := len(s.sessions)
	s.mu.Unlock()
	json.NewEncoder(w).Encode(protocol.HealthResponse{
		Status:   "ok",
		Uptime:   int64(time.Since(s.startTime).Seconds()),
		Sessions: n,
	})
}

// --- sessions CRUD ---

func (s *Server) handleSessions(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		list, err := s.bus.ListSessions()
		if err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		if list == nil {
			list = []map[string]any{}
		}
		json.NewEncoder(w).Encode(list)
	case http.MethodPost:
		var req protocol.CreateSessionRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		if err := req.Validate(); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		if !workspace.IsSafePath(req.CWD, req.CWD) {
			http.Error(w, `{"code":"ERR_PATH_TRAVERSAL"}`, 403)
			return
		}
		sessionID := workspace.NewSessionID()
		workspaceID := workspace.NewID()
		worktreePath, branch, err := workspace.EnsureWorktree(req.CWD, deref(req.TaskName), req.UseWorktree)
		if err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		cwd := req.CWD
		if worktreePath != "" {
			cwd = worktreePath
		}
		// Persist session
		wtVal := worktreePath
		_ = s.bus.UpsertSession(sessionID, workspaceID, string(req.AgentID), cwd, wtVal, branch, string(protocol.StatusRunning))

		// Spawn PTY for shell (agent drivers will replace command when implemented)
		hub := NewHub()
		s.mu.Lock()
		s.sessions[sessionID] = &SessionState{
			SessionID: sessionID, WorkspaceID: workspaceID, AgentID: req.AgentID,
			CWD: cwd, WorktreePath: worktreePath, BranchName: branch,
			Status: protocol.StatusRunning, CreatedAt: protocol.NowMillis(), Hub: hub,
		}
		s.mu.Unlock()

		shell := shellForOS()
		inst, err := s.ptyManager.Spawn(r.Context(), pty.SpawnConfig{
			SessionID: sessionID, Command: shell[0], Args: shell[1:], CWD: cwd,
			Cols: req.Cols, Rows: req.Rows,
		})
		if err != nil {
			log.Printf("[core] pty spawn failed for %s: %v", sessionID, err)
		} else {
			inst.OnData = func(chunk []byte) {
				// Fan-out: binary WS frame + ring buffer already handled + heuristic parser
				frame := protocol.Encode(protocol.OpcodePTYOutput, 0, chunk)
				hub.Broadcast(frame)
				// Non-blocking heuristic scan (spec 02 §5)
				hits := parser.Scan(string(chunk))
				for _, h := range hits {
					payload := map[string]any{"hit": h.Kind, "match": h.Match}
					if _, err := s.bus.AppendEvent(sessionID, string(h.Kind), payload); err != nil {
						log.Printf("[bus] append: %v", err)
					}
					// Also broadcast as JSON-RPC 0x05
					jb, _ := json.Marshal(map[string]any{"type": h.Kind, "sessionId": sessionID, "match": h.Match})
					hub.Broadcast(protocol.Encode(protocol.OpcodeJSONRPC, 0, jb))
				}
			}
			inst.OnExit = func(code int, _ string) {
				_ = s.bus.UpdateSessionStatus(sessionID, string(protocol.StatusStopped))
				s.mu.Lock()
				if st, ok := s.sessions[sessionID]; ok {
					st.Status = protocol.StatusStopped
				}
				s.mu.Unlock()
			}
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(201)
		_ = json.NewEncoder(w).Encode(protocol.CreateSessionResponse{
			SessionID: sessionID, WorkspaceID: workspaceID, WorktreePath: strPtr(worktreePath), Status: protocol.StatusRunning,
		})

	default:
		http.Error(w, "method not allowed", 405)
	}
}

func (s *Server) handleSessionByID(w http.ResponseWriter, r *http.Request) {
	id := strings.TrimPrefix(r.URL.Path, "/api/v1/sessions/")
	if id == "" || strings.Contains(id, "/") {
		http.NotFound(w, r)
		return
	}
	switch r.Method {
	case http.MethodDelete:
		s.ptyManager.Kill(id)
		_ = s.bus.DeleteSession(id)
		s.mu.Lock()
		delete(s.sessions, id)
		s.mu.Unlock()
		w.WriteHeader(204)
	case http.MethodGet:
		s.mu.Lock()
		st, ok := s.sessions[id]
		s.mu.Unlock()
		if !ok {
			http.Error(w, `{"code":"ERR_SESSION_NOT_FOUND"}`, 404)
			return
		}
		// Return catchup if ?since= present
		if sinceStr := r.URL.Query().Get("since"); sinceStr != "" {
			var lastSeq int64
			_, _ = fmt.Sscan(sinceStr, &lastSeq)
			evs, _ := s.bus.GetEventsSince(id, lastSeq)
			json.NewEncoder(w).Encode(evs)
			return
		}
		json.NewEncoder(w).Encode(st)
	default:
		http.Error(w, "method not allowed", 405)
	}
}

func (s *Server) handleApproval(w http.ResponseWriter, r *http.Request) {
	_ = strings.TrimPrefix(r.URL.Path, "/api/v1/approval/")
	var req protocol.ApprovalReply
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), 400)
		return
	}
	// Forward to driver — stub for now
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{"ok": true})
}

func (s *Server) handleQuestion(w http.ResponseWriter, r *http.Request) {
	var req protocol.QuestionReply
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), 400)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{"ok": true})
}

func (s *Server) handleFiles(w http.ResponseWriter, r *http.Request) {
	dir := r.URL.Query().Get("dir")
	if dir == "" {
		http.Error(w, "dir required", 400)
		return
	}
	// TODO: enforce workspace sandbox boundary per session
	entries, err := listFiles(dir)
	if err != nil {
		http.Error(w, err.Error(), 500)
		return
	}
	json.NewEncoder(w).Encode(entries)
}

func (s *Server) handleDiff(w http.ResponseWriter, r *http.Request) {
	sessionID := strings.TrimPrefix(r.URL.Path, "/api/v1/diff/")
	s.mu.Lock()
	st, ok := s.sessions[sessionID]
	s.mu.Unlock()
	if !ok {
		http.Error(w, `{"code":"ERR_SESSION_NOT_FOUND"}`, 404)
		return
	}
	cwd := st.CWD
	// git diff for the worktree
	diff := gitDiff(cwd)
	w.Header().Set("Content-Type", "text/plain")
	_, _ = w.Write([]byte(diff))
}

// --- WebSocket: binary mux (spec 04) ---

func (s *Server) handleWS(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{InsecureSkipVerify: true})
	if err != nil {
		return
	}
	defer conn.Close(websocket.StatusNormalClosure, "")

	// Simple slot assignment: sessionId comes via ?sessionId= query or first JSON-RPC
	sessionID := r.URL.Query().Get("sessionId")
	if sessionID == "" {
		// wait for first catchup or create-session RPC — for now just attach to first session
		s.mu.Lock()
		for id := range s.sessions {
			sessionID = id
			break
		}
		s.mu.Unlock()
	}

	s.mu.Lock()
	st, ok := s.sessions[sessionID]
	s.mu.Unlock()

	// If no session, still accept WS for RPC control plane
	var hub *Hub
	if ok {
		hub = st.Hub
	} else {
		hub = NewHub()
	}

	client := &wsClient{send: make(chan []byte, 64)}
	hub.Add(client)
	defer hub.Remove(client)

	// Writer: hub -> websocket (binary)
	ctx := r.Context()
	go func() {
		for frame := range client.send {
			if err := conn.Write(ctx, websocket.MessageBinary, frame); err != nil {
				return
			}
		}
	}()

	// Replay ring buffer on connect
	if ok {
		if inst, ok2 := s.ptyManager.Get(sessionID); ok2 {
			if data := inst.RingBuffer.ReadAll(); len(data) > 0 {
				_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodePTYOutput, 0, data))
			}
		}
	}

	// Reader: websocket -> PTY / RPC
	for {
		typ, data, err := conn.Read(ctx)
		if err != nil {
			break
		}
		if typ == websocket.MessageText {
			// JSON-RPC control
			var msg map[string]any
			if err := json.Unmarshal(data, &msg); err == nil {
				resp, _ := json.Marshal(map[string]any{"ok": true, "echo": msg})
				_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodeJSONRPC, 0, resp))
			}
			continue
		}
		frame, err := protocol.Decode(data)
		if err != nil {
			continue
		}
		switch frame.Opcode {
		case protocol.OpcodeKeystroke:
			_ = s.ptyManager.Write(sessionID, frame.Payload)
		case protocol.OpcodeViewportResize:
			if cols, rows, err := protocol.DecodeResize(frame.Payload); err == nil {
				s.ptyManager.Resize(sessionID, int(cols), int(rows))
			}
		case protocol.OpcodeCatchup:
			if seq, err := protocol.DecodeCatchup(frame.Payload); err == nil {
				evs, _ := s.bus.GetEventsSince(sessionID, int64(seq))
				for _, ev := range evs {
					jb, _ := json.Marshal(ev)
					_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodeJSONRPC, 0, jb))
				}
			}
		case protocol.OpcodePingPong:
			// echo pong
			_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodePingPong, frame.Slot, frame.Payload))
		case protocol.OpcodeJSONRPC:
			_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodeJSONRPC, frame.Slot, frame.Payload))
		}
	}
}

// --- SSE: lightweight mobile stream ---

func (s *Server) handleSSE(w http.ResponseWriter, r *http.Request) {
	sessionID := r.URL.Query().Get("sessionId")
	if sessionID == "" {
		http.Error(w, "sessionId required", 400)
		return
	}
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", 500)
		return
	}
	// Send lastSeq catchup first
	lastSeqStr := r.URL.Query().Get("lastSeq")
	var lastSeq int64
	if lastSeqStr != "" {
		_, _ = fmt.Sscan(lastSeqStr, &lastSeq)
	}
	if evs, err := s.bus.GetEventsSince(sessionID, lastSeq); err == nil {
		for _, ev := range evs {
			jb, _ := json.Marshal(ev)
			fmt.Fprintf(w, "data: %s\n\n", jb)
		}
		flusher.Flush()
	}
	// Subscribe to hub for live pty
	s.mu.Lock()
	st, ok := s.sessions[sessionID]
	s.mu.Unlock()
	if !ok {
		return
	}
	client := &wsClient{send: make(chan []byte, 64)}
	st.Hub.Add(client)
	defer st.Hub.Remove(client)
	// Keep SSE open until client disconnects; poll hub channel
	ticker := time.NewTicker(25 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-r.Context().Done():
			return
		case frame := <-client.send:
			f, err := protocol.Decode(frame)
			if err != nil {
				continue
			}
			if f.Opcode == protocol.OpcodePTYOutput {
				fmt.Fprintf(w, "event: chunk\ndata: %s\n\n", jsonEscape(string(f.Payload)))
			} else {
				fmt.Fprintf(w, "data: %s\n\n", string(f.Payload))
			}
			flusher.Flush()
		case <-ticker.C:
			fmt.Fprintf(w, ": keepalive\n\n")
			flusher.Flush()
		}
	}
}

// --- helpers ---

func deref(s *string) string { if s == nil { return "" }; return *s }
func strPtr(s string) *string { if s == "" { return nil }; return &s }

func shellForOS() []string {
	// On Windows prefer cmd.exe; on Unix prefer $SHELL or /bin/sh
	if isWindows() {
		return []string{"cmd.exe"}
	}
	if sh := getenv("SHELL"); sh != "" {
		return []string{sh}
	}
	return []string{"/bin/sh"}
}
func isWindows() bool { return filepath.Separator == '\\' }
func getenv(k string) string {
	for _, kv := range os.Environ() {
		parts := strings.SplitN(kv, "=", 2)
		if len(parts) == 2 && parts[0] == k {
			return parts[1]
		}
	}
	return ""
}

func listFiles(dir string) ([]protocol.FileEntry, error) {
	// implemented in files.go
	return listFilesImpl(dir)
}
func gitDiff(cwd string) string { return gitDiffImpl(cwd) }

func jsonEscape(s string) string { b, _ := json.Marshal(s); return string(b) }

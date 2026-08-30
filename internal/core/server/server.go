package server

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/coder/websocket"

	"github.com/morewebs/OpenRemote/internal/core/approval"
	"github.com/morewebs/OpenRemote/internal/core/auth"
	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/parser"
	"github.com/morewebs/OpenRemote/internal/core/rpc"
	"github.com/morewebs/OpenRemote/internal/core/tunnel"
	"github.com/morewebs/OpenRemote/internal/core/workspace"
	"github.com/morewebs/OpenRemote/internal/driver"
	"github.com/morewebs/OpenRemote/internal/protocol"
	"github.com/morewebs/OpenRemote/internal/pty"
	"github.com/morewebs/OpenRemote/internal/telegram"
)

type Config struct {
	Addr           string   // 127.0.0.1:4097 default
	DataDir        string   // ~/.openremote/data
	Token          string   // empty = no auth (dev)
	AllowedRoots   []string // allowed root directories for workspace sandbox
	TelegramToken  string
	TelegramChatID int64
}

type Server struct {
	cfg        Config
	bus        *events.Bus
	ptyManager *pty.Manager
	drivers    *driver.Registry
	approvals  *approval.Registry

	pendingMu        sync.RWMutex
	pendingQuestions map[string]string
	tunnels          *tunnel.Manager
	telegram         *telegram.Bot
	rateLimiter      *auth.RateLimiter
	rpcMux           *rpc.Mux
	http             *http.Server
	mu               sync.RWMutex
	startTime        time.Time
	sessions         map[string]*SessionState
}

type SessionState struct {
	SessionID    string                 `json:"sessionId"`
	WorkspaceID  string                 `json:"workspaceId"`
	AgentID      protocol.AgentID       `json:"agentId"`
	CWD          string                 `json:"cwd"`
	OriginCWD    string                 `json:"originCwd,omitempty"`
	WorktreePath string                 `json:"worktreePath,omitempty"`
	BranchName   string                 `json:"branchName,omitempty"`
	Status       protocol.SessionStatus `json:"status"`
	CreatedAt    int64                  `json:"createdAt"`
	DriverSess   driver.Session         `json:"-"`
	Parser       *parser.StreamParser   `json:"-"`
	Hub          *Hub                   `json:"-"`
}

type Hub struct {
	mu      sync.RWMutex
	clients map[*wsClient]struct{}
}

type wsClient struct {
	send chan []byte
	done chan struct{}
	once sync.Once
}

func (c *wsClient) Close() {
	c.once.Do(func() {
		close(c.done)
	})
}

func NewHub() *Hub             { return &Hub{clients: make(map[*wsClient]struct{})} }
func (h *Hub) Add(c *wsClient) { h.mu.Lock(); h.clients[c] = struct{}{}; h.mu.Unlock() }
func (h *Hub) Remove(c *wsClient) {
	h.mu.Lock()
	delete(h.clients, c)
	h.mu.Unlock()
	c.Close()
}
func (h *Hub) Broadcast(frame []byte) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	for c := range h.clients {
		select {
		case c.send <- frame:
		default:
		}
	}
}

func New(cfg Config, bus *events.Bus) *Server {
	if cfg.Addr == "" {
		cfg.Addr = "127.0.0.1:4097"
	}
	if len(cfg.AllowedRoots) == 0 {
		if home, err := os.UserHomeDir(); err == nil {
			cfg.AllowedRoots = []string{home}
		}
	}

	ptyMgr := pty.NewManager()
	drvRegistry := driver.NewRegistry(ptyMgr)

	s := &Server{
		cfg:              cfg,
		bus:              bus,
		ptyManager:       ptyMgr,
		drivers:          drvRegistry,
		tunnels:          tunnel.NewManager(),
		rateLimiter:      auth.NewRateLimiter(50, 100),
		rpcMux:           rpc.NewMux(),
		sessions:         make(map[string]*SessionState),
		pendingQuestions: make(map[string]string),
		startTime:        time.Now(),
	}

	s.approvals = approval.NewRegistry(func(app *approval.PendingApproval) {
		// On timeout auto-deny
		s.handleApprovalExpired(app)
	})

	s.telegram = telegram.New(telegram.Config{
		Token: cfg.TelegramToken,
	}, bus, s.approvals)

	s.setupRPC()
	s.Restore()

	return s
}

func (s *Server) Restore() {
	if s.bus == nil {
		return
	}
	list, err := s.bus.ListSessions()
	if err != nil {
		log.Printf("[core] session restore warning: %v", err)
		return
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	for _, row := range list {
		sID, _ := row["sessionId"].(string)
		wID, _ := row["workspaceId"].(string)
		aID, _ := row["agentId"].(string)
		cwd, _ := row["cwd"].(string)
		origCWD, _ := row["originCwd"].(string)
		wt, _ := row["worktreePath"].(string)
		br, _ := row["branchName"].(string)
		created, _ := row["createdAt"].(int64)

		if sID == "" {
			continue
		}

		// Since daemon restarted, previous child process is stopped
		_ = s.bus.UpdateSessionStatus(sID, string(protocol.StatusStopped))

		s.sessions[sID] = &SessionState{
			SessionID:    sID,
			WorkspaceID:  wID,
			AgentID:      protocol.AgentID(aID),
			CWD:          cwd,
			OriginCWD:    origCWD,
			WorktreePath: wt,
			BranchName:   br,
			Status:       protocol.StatusStopped,
			CreatedAt:    created,
			Parser:       parser.NewStreamParser(sID),
			Hub:          NewHub(),
		}
	}
	log.Printf("[core] restored %d session records from database", len(s.sessions))
}

func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", s.handleHealth)
	mux.HandleFunc("/ws", s.handleWS)
	mux.HandleFunc("/events", s.handleSSE)

	mux.HandleFunc("/api/v1/agents", s.handleAgents)
	mux.HandleFunc("/api/v1/sessions", s.handleSessions)
	mux.HandleFunc("/api/v1/sessions/", s.handleSessionByID)
	mux.HandleFunc("/api/v1/approval/", s.handleApproval)
	mux.HandleFunc("/api/v1/question/", s.handleQuestion)
	mux.HandleFunc("/api/v1/files", s.handleFiles)
	mux.HandleFunc("/api/v1/diff/", s.handleDiff)
	mux.HandleFunc("/api/v1/tunnels", s.handleTunnels)
	mux.HandleFunc("/api/v1/telegram/status", s.handleTelegramStatus)

	// Fallback/SPA Static Handler for Flutter Companion web client
	mux.Handle("/", StaticHandler())

	var h http.Handler = mux
	h = auth.Middleware(s.cfg.Token, s.rateLimiter, h)
	h = corsMiddleware(h)
	return h
}

func corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		origin := r.Header.Get("Origin")
		if origin != "" {
			w.Header().Set("Access-Control-Allow-Origin", origin)
			w.Header().Set("Access-Control-Allow-Headers", "Authorization, Content-Type, X-Requested-With")
			w.Header().Set("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS")
			w.Header().Set("Access-Control-Allow-Credentials", "true")
			w.Header().Set("Vary", "Origin")
		}
		if r.Method == http.MethodOptions {
			w.WriteHeader(204)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func (s *Server) ListenAndServe() error {
	if s.cfg.TelegramToken != "" {
		_ = s.telegram.Start(context.Background())
	}
	s.http = &http.Server{
		Addr:              s.cfg.Addr,
		Handler:           s.Handler(),
		ReadHeaderTimeout: 5 * time.Second,
	}
	log.Printf("[core] listening on http://%s  data=%s", s.cfg.Addr, s.cfg.DataDir)
	return s.http.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	if s.tunnels != nil {
		_ = s.tunnels.Stop()
	}
	if s.http != nil {
		return s.http.Shutdown(ctx)
	}
	return nil
}

// --- Health ---

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	s.mu.RLock()
	n := len(s.sessions)
	s.mu.RUnlock()
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(protocol.HealthResponse{
		Status:   "ok",
		Uptime:   int64(time.Since(s.startTime).Seconds()),
		Sessions: n,
	})
}

// --- Agents ---

func (s *Server) handleAgents(w http.ResponseWriter, r *http.Request) {
	list := s.drivers.List()
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(list)
}

// --- Sessions CRUD ---

func (s *Server) handleSessions(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		s.mu.RLock()
		var list []map[string]any
		for _, st := range s.sessions {
			list = append(list, map[string]any{
				"sessionId":    st.SessionID,
				"workspaceId":  st.WorkspaceID,
				"agentId":      st.AgentID,
				"cwd":          st.CWD,
				"worktreePath": st.WorktreePath,
				"branchName":   st.BranchName,
				"status":       st.Status,
				"createdAt":    st.CreatedAt,
			})
		}
		s.mu.RUnlock()
		if list == nil {
			list = []map[string]any{}
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(list)

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

		// Verify allowed root
		if !workspace.IsSafePathAny(s.cfg.AllowedRoots, req.CWD) {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusForbidden)
			fmt.Fprint(w, `{"code":"ERR_PATH_TRAVERSAL","message":"requested directory outside allowed roots"}`)
			return
		}

		driverInst, ok := s.drivers.Get(req.AgentID)
		if !ok {
			http.Error(w, fmt.Sprintf("unsupported agent %q", req.AgentID), 400)
			return
		}

		sessionID := workspace.NewSessionID()
		workspaceID := workspace.NewID()
		worktreePath, branch, err := workspace.EnsureWorktree(req.CWD, deref(req.TaskName), req.UseWorktree)
		if err != nil {
			http.Error(w, err.Error(), 500)
			return
		}

		targetCWD := req.CWD
		if worktreePath != "" {
			targetCWD = worktreePath
		}

		_ = s.bus.UpsertSession(sessionID, workspaceID, string(req.AgentID), targetCWD, req.CWD, worktreePath, branch, string(protocol.StatusRunning))

		hub := NewHub()
		streamParser := parser.NewStreamParser(sessionID)

		st := &SessionState{
			SessionID:    sessionID,
			WorkspaceID:  workspaceID,
			AgentID:      req.AgentID,
			CWD:          targetCWD,
			OriginCWD:    req.CWD,
			WorktreePath: worktreePath,
			BranchName:   branch,
			Status:       protocol.StatusRunning,
			CreatedAt:    protocol.NowMillis(),
			Parser:       streamParser,
			Hub:          hub,
		}

		s.mu.Lock()
		s.sessions[sessionID] = st
		s.mu.Unlock()

		// Sink implementation bridging driver to event bus, websockets, and parser
		sink := &serverSink{
			server:    s,
			sessionID: sessionID,
			hub:       hub,
			parser:    streamParser,
		}

		drvSess, err := driverInst.Start(context.Background(), driver.SessionConfig{
			SessionID:     sessionID,
			AgentID:       req.AgentID,
			CWD:           req.CWD,
			WorktreePath:  worktreePath,
			Cols:          req.Cols,
			Rows:          req.Rows,
			TaskName:      deref(req.TaskName),
			RemoteControl: req.RemoteControl,
		}, sink)

		if err != nil {
			log.Printf("[core] failed to start driver %s: %v", req.AgentID, err)
			_ = s.bus.UpdateSessionStatus(sessionID, string(protocol.StatusStopped))
			st.Status = protocol.StatusStopped
		} else {
			st.DriverSess = drvSess
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(201)
		_ = json.NewEncoder(w).Encode(protocol.CreateSessionResponse{
			SessionID:    sessionID,
			WorkspaceID:  workspaceID,
			WorktreePath: strPtr(worktreePath),
			Status:       st.Status,
		})

	default:
		http.Error(w, "method not allowed", 405)
	}
}

func (s *Server) handleSessionByID(w http.ResponseWriter, r *http.Request) {
	relPath := strings.TrimPrefix(r.URL.Path, "/api/v1/sessions/")
	parts := strings.Split(relPath, "/")
	id := parts[0]

	if id == "" {
		http.NotFound(w, r)
		return
	}

	// Handle /api/v1/sessions/:id/prompt
	if len(parts) == 2 && parts[1] == "prompt" && r.Method == http.MethodPost {
		var req struct {
			Prompt string `json:"prompt"`
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		s.mu.RLock()
		st, ok := s.sessions[id]
		s.mu.RUnlock()
		if !ok || st.DriverSess == nil {
			http.Error(w, `{"code":"ERR_SESSION_NOT_FOUND"}`, 404)
			return
		}
		if err := st.DriverSess.Prompt(req.Prompt); err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
		return
	}

	switch r.Method {
	case http.MethodDelete:
		s.mu.Lock()
		st, ok := s.sessions[id]
		delete(s.sessions, id)
		s.mu.Unlock()

		if ok {
			if st.DriverSess != nil {
				_ = st.DriverSess.Close()
			}
			s.ptyManager.Kill(id)
			if st.WorktreePath != "" && st.OriginCWD != "" {
				_ = workspace.RemoveWorktree(st.OriginCWD, st.WorktreePath)
			}
		}
		_ = s.bus.DeleteSession(id)
		w.WriteHeader(204)

	case http.MethodGet:
		s.mu.RLock()
		st, ok := s.sessions[id]
		s.mu.RUnlock()
		if !ok {
			http.Error(w, `{"code":"ERR_SESSION_NOT_FOUND"}`, 404)
			return
		}
		if sinceStr := r.URL.Query().Get("since"); sinceStr != "" {
			var lastSeq int64
			_, _ = fmt.Sscan(sinceStr, &lastSeq)
			evs, _ := s.bus.GetEventsSince(id, lastSeq)
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(evs)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(st)

	default:
		http.Error(w, "method not allowed", 405)
	}
}

// --- Approvals & Questions ---

// broadcastEvent persists an event to the bus and pushes it to all live
// WebSocket clients attached to the session's hub.
func (s *Server) broadcastEvent(sessionID, evType string, evt any) {
	seq, _ := s.bus.AppendEvent(sessionID, evType, evt)
	jb, _ := json.Marshal(evt)
	var withSeq map[string]any
	if err := json.Unmarshal(jb, &withSeq); err == nil {
		withSeq["seq"] = seq
		jb, _ = json.Marshal(withSeq)
	}
	s.mu.RLock()
	st, ok := s.sessions[sessionID]
	s.mu.RUnlock()
	if ok && st.Hub != nil {
		st.Hub.Broadcast(protocol.Encode(protocol.OpcodeJSONRPC, 0, jb))
	}
}

func (s *Server) handleApproval(w http.ResponseWriter, r *http.Request) {
	appID := strings.TrimPrefix(r.URL.Path, "/api/v1/approval/")
	if appID == "" {
		http.Error(w, "approval ID required", 400)
		return
	}

	var req protocol.ApprovalReply
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), 400)
		return
	}

	app, err := s.approvals.Resolve(appID, req.Approved, "user")
	if err != nil {
		http.Error(w, err.Error(), 404)
		return
	}

	s.mu.RLock()
	st, ok := s.sessions[app.SessionID]
	s.mu.RUnlock()

	if ok && st.DriverSess != nil {
		_ = st.DriverSess.Approve(appID, req.Approved)
	}

	evt := protocol.ApprovalResolvedEvent{
		BaseEvent: protocol.BaseEvent{
			SessionID: app.SessionID,
			Timestamp: protocol.NowMillis(),
		},
		Type:       protocol.EventApprovalResolved,
		ApprovalID: appID,
		Approved:   req.Approved,
		ResolvedBy: "user",
	}
	s.broadcastEvent(app.SessionID, string(evt.Type), evt)

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{"ok": true, "approved": req.Approved})
}

func (s *Server) handleApprovalExpired(app *approval.PendingApproval) {
	s.mu.RLock()
	st, ok := s.sessions[app.SessionID]
	s.mu.RUnlock()

	if ok && st.DriverSess != nil {
		_ = st.DriverSess.Approve(app.ID, false)
	}

	evt := protocol.ApprovalResolvedEvent{
		BaseEvent: protocol.BaseEvent{
			SessionID: app.SessionID,
			Timestamp: protocol.NowMillis(),
		},
		Type:       protocol.EventApprovalResolved,
		ApprovalID: app.ID,
		Approved:   false,
		ResolvedBy: "timeout",
	}
	s.broadcastEvent(app.SessionID, string(evt.Type), evt)
}

func (s *Server) handleQuestion(w http.ResponseWriter, r *http.Request) {
	qID := strings.TrimPrefix(r.URL.Path, "/api/v1/question/")
	if qID == "" {
		http.Error(w, "question ID required", 400)
		return
	}

	var req protocol.QuestionReply
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, err.Error(), 400)
		return
	}

	// Forward the answer to the owning driver session so it reaches the
	// agent's stdin (multiple-choice selection, free text, ...).
	s.pendingMu.RLock()
	sessionID, known := s.pendingQuestions[qID]
	s.pendingMu.RUnlock()

	if known {
		s.mu.RLock()
		st, ok := s.sessions[sessionID]
		s.mu.RUnlock()
		if ok && st.DriverSess != nil {
			// Forward a scalar for single-answer questions so drivers can
			// write the raw selection into the agent's stdin.
			var payload any = req.Answers
			if len(req.Answers) == 1 {
				payload = req.Answers[0]
			}
			_ = st.DriverSess.Answer(qID, payload)
		}
	}

	evt := protocol.QuestionAnsweredEvent{
		BaseEvent: protocol.BaseEvent{
			SessionID: sessionID,
			Timestamp: protocol.NowMillis(),
		},
		Type:       protocol.EventQuestionAnswered,
		QuestionID: qID,
		Answers:    req.Answers,
	}
	if sessionID != "" {
		s.broadcastEvent(sessionID, string(evt.Type), evt)
	}

	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
}

// --- Files & Diff ---

func (s *Server) handleFiles(w http.ResponseWriter, r *http.Request) {
	dir := r.URL.Query().Get("dir")
	if dir == "" {
		http.Error(w, "dir parameter required", 400)
		return
	}

	if !workspace.IsSafePathAny(s.cfg.AllowedRoots, dir) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusForbidden)
		fmt.Fprint(w, `{"code":"ERR_PATH_TRAVERSAL"}`)
		return
	}

	entries, err := listFiles(dir)
	if err != nil {
		http.Error(w, err.Error(), 500)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(entries)
}

func (s *Server) handleDiff(w http.ResponseWriter, r *http.Request) {
	sessionID := strings.TrimPrefix(r.URL.Path, "/api/v1/diff/")
	s.mu.RLock()
	st, ok := s.sessions[sessionID]
	s.mu.RUnlock()
	if !ok {
		http.Error(w, `{"code":"ERR_SESSION_NOT_FOUND"}`, 404)
		return
	}

	diff := gitDiff(st.CWD)
	w.Header().Set("Content-Type", "text/plain")
	_, _ = w.Write([]byte(diff))
}

// --- Tunnels & Telegram ---

func (s *Server) handleTunnels(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		list := s.tunnels.List()
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(list)
	case http.MethodPost:
		var req struct {
			Name   string `json:"name"`
			Action string `json:"action"` // "start" or "stop"
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		if req.Action == "stop" {
			_ = s.tunnels.Stop()
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
			return
		}
		u, err := s.tunnels.Start(r.Context(), req.Name, s.cfg.Addr)
		if err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true, "url": u})
	}
}

func (s *Server) handleTelegramStatus(w http.ResponseWriter, r *http.Request) {
	st := s.telegram.Status()
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(st)
}

// --- WebSocket ---

func (s *Server) handleWS(w http.ResponseWriter, r *http.Request) {
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		OriginPatterns: []string{"*"},
	})
	if err != nil {
		return
	}
	defer conn.Close(websocket.StatusNormalClosure, "")

	sessionID := r.URL.Query().Get("sessionId")
	s.mu.RLock()
	st, ok := s.sessions[sessionID]
	s.mu.RUnlock()

	var hub *Hub
	if ok {
		hub = st.Hub
	} else {
		hub = NewHub()
	}

	client := &wsClient{
		send: make(chan []byte, 128),
		done: make(chan struct{}),
	}
	hub.Add(client)
	defer hub.Remove(client)

	ctx := r.Context()

	// Writer goroutine
	go func() {
		for {
			select {
			case <-client.done:
				return
			case <-ctx.Done():
				return
			case frame, open := <-client.send:
				if !open {
					return
				}
				if err := conn.Write(ctx, websocket.MessageBinary, frame); err != nil {
					return
				}
			}
		}
	}()

	// Replay PTY ring buffer if session has active terminal
	if ok {
		if term, isTerm := st.DriverSess.(driver.Terminal); isTerm {
			if snap := term.Snapshot(); len(snap) > 0 {
				_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodePTYOutput, 0, snap))
			}
		}
	}

	// Reader loop
	for {
		typ, data, err := conn.Read(ctx)
		if err != nil {
			break
		}

		if typ == websocket.MessageText {
			// Handle JSON-RPC over text frame
			respBytes, _ := s.rpcMux.Dispatch(ctx, sessionID, data)
			_ = conn.Write(ctx, websocket.MessageText, respBytes)
			continue
		}

		frame, err := protocol.Decode(data)
		if err != nil {
			continue
		}

		switch frame.Opcode {
		case protocol.OpcodeKeystroke:
			s.mu.RLock()
			st, ok := s.sessions[sessionID]
			s.mu.RUnlock()
			if ok && st.DriverSess != nil {
				if term, isTerm := st.DriverSess.(driver.Terminal); isTerm {
					_ = term.RawInput(frame.Payload)
				}
			}

		case protocol.OpcodeViewportResize:
			if cols, rows, err := protocol.DecodeResize(frame.Payload); err == nil {
				s.mu.RLock()
				st, ok := s.sessions[sessionID]
				s.mu.RUnlock()
				if ok && st.DriverSess != nil {
					if term, isTerm := st.DriverSess.(driver.Terminal); isTerm {
						_ = term.Resize(int(cols), int(rows))
					}
				}
			}

		case protocol.OpcodeCatchup:
			if seq, err := protocol.DecodeCatchup(frame.Payload); err == nil {
				evs, _ := s.bus.GetEventsSince(sessionID, int64(seq))
				for _, ev := range evs {
					jb, _ := json.Marshal(ev)
					_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodeJSONRPC, frame.Slot, jb))
				}
			}

		case protocol.OpcodePingPong:
			_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodePingPong, frame.Slot, frame.Payload))

		case protocol.OpcodeJSONRPC:
			respBytes, _ := s.rpcMux.Dispatch(ctx, sessionID, frame.Payload)
			_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodeJSONRPC, frame.Slot, respBytes))
		}
	}
}

// --- SSE ---

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

	s.mu.RLock()
	st, ok := s.sessions[sessionID]
	s.mu.RUnlock()
	if !ok {
		return
	}

	client := &wsClient{send: make(chan []byte, 64), done: make(chan struct{})}
	st.Hub.Add(client)
	defer st.Hub.Remove(client)

	ticker := time.NewTicker(20 * time.Second)
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

// --- RPC Handlers ---

func (s *Server) setupRPC() {
	s.rpcMux.Register("session.list", func(ctx context.Context, sessionID string, params json.RawMessage) (any, *rpc.RPCError) {
		s.mu.RLock()
		defer s.mu.RUnlock()
		var list []map[string]any
		for _, st := range s.sessions {
			list = append(list, map[string]any{
				"sessionId": st.SessionID,
				"agentId":   st.AgentID,
				"status":    st.Status,
			})
		}
		return list, nil
	})

	s.rpcMux.Register("prompt.send", func(ctx context.Context, sessionID string, params json.RawMessage) (any, *rpc.RPCError) {
		var req struct {
			Prompt string `json:"prompt"`
		}
		if err := json.Unmarshal(params, &req); err != nil {
			return nil, &rpc.RPCError{Code: rpc.ErrInvalidParams, Message: err.Error()}
		}
		s.mu.RLock()
		st, ok := s.sessions[sessionID]
		s.mu.RUnlock()
		if !ok || st.DriverSess == nil {
			return nil, &rpc.RPCError{Code: rpc.ErrInternalError, Message: "session not found"}
		}
		if err := st.DriverSess.Prompt(req.Prompt); err != nil {
			return nil, &rpc.RPCError{Code: rpc.ErrInternalError, Message: err.Error()}
		}
		return map[string]any{"ok": true}, nil
	})

	s.rpcMux.Register("approval.resolve", func(ctx context.Context, sessionID string, params json.RawMessage) (any, *rpc.RPCError) {
		var req struct {
			ApprovalID string `json:"approvalId"`
			Approved   bool   `json:"approved"`
		}
		if err := json.Unmarshal(params, &req); err != nil {
			return nil, &rpc.RPCError{Code: rpc.ErrInvalidParams, Message: err.Error()}
		}
		app, err := s.approvals.Resolve(req.ApprovalID, req.Approved, "rpc")
		if err != nil {
			return nil, &rpc.RPCError{Code: rpc.ErrInternalError, Message: err.Error()}
		}
		s.mu.RLock()
		st, ok := s.sessions[app.SessionID]
		s.mu.RUnlock()
		if ok && st.DriverSess != nil {
			_ = st.DriverSess.Approve(req.ApprovalID, req.Approved)
		}
		return map[string]any{"ok": true}, nil
	})

	s.rpcMux.Register("agents.list", func(ctx context.Context, sessionID string, params json.RawMessage) (any, *rpc.RPCError) {
		return s.drivers.List(), nil
	})
}

// --- serverSink implements driver.Sink ---

type serverSink struct {
	server    *Server
	sessionID string
	hub       *Hub
	parser    *parser.StreamParser
}

func (s *serverSink) Bytes(data []byte) {
	frame := protocol.Encode(protocol.OpcodePTYOutput, 0, data)
	s.hub.Broadcast(frame)

	// Feed to parser
	events := s.parser.FeedLine(string(data))
	for _, ev := range events {
		if appEvt, ok := ev.(protocol.ApprovalRequestedEvent); ok {
			s.server.approvals.Put(&approval.PendingApproval{
				ID:                appEvt.ApprovalID,
				SessionID:         s.sessionID,
				ToolName:          appEvt.ToolName,
				Command:           appEvt.Command,
				AutoDenyTimeoutMs: appEvt.AutoDenyTimeoutMs,
			})
			if s.server.cfg.TelegramChatID != 0 {
				if app, ok2 := s.server.approvals.Get(appEvt.ApprovalID); ok2 {
					s.server.telegram.NotifyApproval(context.Background(), s.server.cfg.TelegramChatID, app)
				}
			}
		}
		if qEvt, ok := ev.(protocol.QuestionAskedEvent); ok {
			s.server.pendingMu.Lock()
			s.server.pendingQuestions[qEvt.QuestionID] = s.sessionID
			s.server.pendingMu.Unlock()
		}
		s.Event(ev)
	}
}

func (s *serverSink) Message(msg chat.Message) {
	evt := protocol.ChatMessageEvent{
		BaseEvent: protocol.BaseEvent{
			SessionID: s.sessionID,
			Timestamp: msg.Timestamp,
		},
		Type:      protocol.EventChatMessage,
		MessageID: msg.ID,
		Role:      msg.Role,
		Kind:      msg.Kind,
		Text:      msg.Text,
		ToolName:  msg.ToolName,
		Meta:      msg.Meta,
		Streaming: msg.Streaming,
		Rev:       msg.Rev,
	}

	seq, _ := s.server.bus.AppendEvent(s.sessionID, string(evt.Type), evt)
	evt.Seq = seq

	jb, _ := json.Marshal(evt)
	s.hub.Broadcast(protocol.Encode(protocol.OpcodeJSONRPC, 0, jb))

	if !msg.Streaming && s.server.cfg.TelegramChatID != 0 {
		s.server.telegram.NotifyChatMessage(context.Background(), s.server.cfg.TelegramChatID, msg)
	}
}

func (s *serverSink) Event(evt any) {
	var evType string
	if b, ok := evt.(protocol.AgentEvent); ok {
		evType = string(b.Type)
	} else if ab, ok := evt.(protocol.ApprovalRequestedEvent); ok {
		evType = string(ab.Type)
	} else if q, ok := evt.(protocol.QuestionAskedEvent); ok {
		evType = string(q.Type)
	} else if d, ok := evt.(protocol.DiffGeneratedEvent); ok {
		evType = string(d.Type)
	} else if t, ok := evt.(protocol.TurnCompletedEvent); ok {
		evType = string(t.Type)
	} else if au, ok := evt.(protocol.AuthURLEvent); ok {
		evType = string(au.Type)
	} else {
		evType = "custom.event"
	}

	seq, _ := s.server.bus.AppendEvent(s.sessionID, evType, evt)
	jb, _ := json.Marshal(evt)
	var withSeq map[string]any
	if err := json.Unmarshal(jb, &withSeq); err == nil {
		withSeq["seq"] = seq
		jb, _ = json.Marshal(withSeq)
	}
	s.hub.Broadcast(protocol.Encode(protocol.OpcodeJSONRPC, 0, jb))
}

func (s *serverSink) Exit(code int, signal string) {
	_ = s.server.bus.UpdateSessionStatus(s.sessionID, string(protocol.StatusStopped))
	s.server.mu.Lock()
	if st, ok := s.server.sessions[s.sessionID]; ok {
		st.Status = protocol.StatusStopped
	}
	s.server.mu.Unlock()

	statusEvt := protocol.SessionStatusEvent{
		BaseEvent: protocol.BaseEvent{
			SessionID: s.sessionID,
			Timestamp: protocol.NowMillis(),
		},
		Type:   protocol.EventSessionStatus,
		Status: protocol.StatusStopped,
		Reason: fmt.Sprintf("exited with code %d %s", code, signal),
	}
	s.Event(statusEvt)
}

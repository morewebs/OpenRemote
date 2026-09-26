package server

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
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
	Addr                 string   // 127.0.0.1:4097 default
	DataDir              string   // ~/.openremote/data
	Token                string   // empty = no auth (dev)
	AllowedRoots         []string // allowed root directories for workspace sandbox
	TelegramToken        string
	TelegramChatID       int64
	TelegramAllowedUsers []int64
	TelegramTopics       bool
	AllowedOrigin        string // exact http(s) origin trusted for WebSocket handshake (CSWSH defense); defaults to http://<Addr>
	WorkerBinary         string // daemon executable used for isolated PTY workers; empty in tests
}

type Server struct {
	ctx        context.Context
	cancel     context.CancelFunc
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

// snapshotSession copies mutable fields while the server lock is held.
// Drivers and hubs have their own synchronization and may be used afterwards.
func (s *Server) snapshotSession(id string) (*SessionState, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	state, ok := s.sessions[id]
	if !ok {
		return nil, false
	}
	copy := *state
	return &copy, true
}

type Hub struct {
	eventMu sync.Mutex // persistence and publication share the same order
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

func (h *Hub) Close() {
	h.mu.Lock()
	defer h.mu.Unlock()
	for c := range h.clients {
		c.Close()
		delete(h.clients, c)
	}
}
func (h *Hub) Broadcast(frame []byte) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	for c := range h.clients {
		select {
		case c.send <- frame:
		default:
			// Force a reconnect and durable catchup instead of silently losing data.
			c.Close()
		}
	}
}

// reANSI strips common ANSI escape sequences so the parser sees clean text.
var reANSI = regexp.MustCompile("\x1b(?:\\[[0-9;?]*[A-Za-z]|\\].*?\x07|\\(B)")

func stripANSI(s string) string { return reANSI.ReplaceAllString(s, "") }

func New(cfg Config, bus *events.Bus) *Server {
	if cfg.Addr == "" {
		cfg.Addr = "127.0.0.1:4097"
	}
	if len(cfg.AllowedRoots) == 0 {
		if cwd, err := os.Getwd(); err == nil {
			cfg.AllowedRoots = []string{cwd}
		}
	}

	ptyMgr := pty.NewManager()
	if cfg.WorkerBinary != "" {
		ptyMgr = pty.NewIsolatedManager(cfg.WorkerBinary, "pty-worker")
	}
	drvRegistry := driver.NewRegistry(ptyMgr)

	ctx, cancel := context.WithCancel(context.Background())
	s := &Server{
		ctx:              ctx,
		cancel:           cancel,
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
		Token:          cfg.TelegramToken,
		AllowedUserIDs: cfg.TelegramAllowedUsers,
		DefaultChatID:  cfg.TelegramChatID,
		ForumTopics:    cfg.TelegramTopics,
		Handlers:       s.telegramHandlers(),
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
	mux.HandleFunc("/api/v1/file", s.handleFile)
	mux.HandleFunc("/api/v1/diff/", s.handleDiff)
	mux.HandleFunc("/api/v1/tunnels", s.handleTunnels)
	mux.HandleFunc("/api/v1/telegram/status", s.handleTelegramStatus)

	// Fallback/SPA Static Handler for Flutter Companion web client
	mux.Handle("/", StaticHandler())

	var h http.Handler = mux
	h = auth.Middleware(s.cfg.Token, s.rateLimiter, h)
	h = corsMiddleware(h, s.cfg.AllowedOrigin)
	return h
}

func corsMiddleware(next http.Handler, configuredOrigin string) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		origin := r.Header.Get("Origin")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		w.Header().Set("Referrer-Policy", "no-referrer")
		w.Header().Set("X-Frame-Options", "DENY")
		if origin != "" && isAllowedOrigin(origin, r.Host, configuredOrigin) {
			w.Header().Set("Access-Control-Allow-Origin", origin)
			w.Header().Set("Access-Control-Allow-Headers", "Authorization, Content-Type, X-Requested-With")
			w.Header().Set("Access-Control-Allow-Methods", "GET, POST, DELETE, OPTIONS")
			w.Header().Set("Access-Control-Allow-Credentials", "true")
			w.Header().Add("Vary", "Origin")
		} else if origin != "" {
			w.Header().Add("Vary", "Origin")
			http.Error(w, "origin not allowed", http.StatusForbidden)
			return
		}
		if r.Method == http.MethodOptions {
			w.WriteHeader(204)
			return
		}
		next.ServeHTTP(w, r)
	})
}

func isAllowedOrigin(origin, host, configured string) bool {
	u, err := url.Parse(origin)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.User != nil || u.Host == "" || u.Path != "" || u.RawQuery != "" || u.Fragment != "" {
		return false
	}
	if u.Host == host || origin == configured {
		return true
	}
	local := func(name string) bool { return name == "localhost" || name == "127.0.0.1" || name == "::1" }
	requestURL, _ := url.Parse("http://" + host)
	return requestURL != nil && local(requestURL.Hostname()) && local(u.Hostname())
}

func (s *Server) ListenAndServe() error {
	if s.cfg.TelegramToken != "" {
		if err := s.telegram.Start(s.ctx); err != nil {
			log.Printf("[telegram] %v", err)
		}
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
	s.telegram.Close()
	s.mu.RLock()
	var sessions []driver.Session
	for _, st := range s.sessions {
		if st.DriverSess != nil {
			sessions = append(sessions, st.DriverSess)
		}
		st.Hub.Close()
	}
	s.mu.RUnlock()
	for _, session := range sessions {
		_ = session.Close()
	}
	s.ptyManager.Close()
	s.cancel()
	s.rateLimiter.Stop()
	s.approvals.Close()
	if s.tunnels != nil {
		_ = s.tunnels.Stop()
	}
	if s.http != nil {
		return s.http.Shutdown(ctx)
	}
	return nil
}

// methodNotAllowed replies 405 with the verbs the route does support, so a
// spec route never silently answers a request it does not implement.
func methodNotAllowed(w http.ResponseWriter, allowed ...string) {
	w.Header().Set("Allow", strings.Join(allowed, ", "))
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusMethodNotAllowed)
	_, _ = fmt.Fprintf(w, `{"code":"ERR_METHOD_NOT_ALLOWED","message":"method not allowed; expected %s"}`, strings.Join(allowed, " or "))
}

// --- Health ---

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		methodNotAllowed(w, http.MethodGet, http.MethodHead)
		return
	}
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
	if r.Method != http.MethodGet {
		methodNotAllowed(w, http.MethodGet)
		return
	}
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
		if s.ctx.Err() != nil {
			http.Error(w, "server shutting down", http.StatusServiceUnavailable)
			return
		}
		var req protocol.CreateSessionRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		if err := req.Validate(); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		absoluteCWD, err := filepath.Abs(req.CWD)
		if err != nil {
			http.Error(w, "invalid working directory", http.StatusBadRequest)
			return
		}
		req.CWD = absoluteCWD
		if info, err := os.Stat(req.CWD); err != nil || !info.IsDir() {
			http.Error(w, "working directory does not exist", http.StatusBadRequest)
			return
		}

		// Verify allowed root
		if !workspace.IsSafePathAny(s.cfg.AllowedRoots, req.CWD) {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusForbidden)
			_, _ = fmt.Fprint(w, `{"code":"ERR_PATH_TRAVERSAL","message":"requested directory outside allowed roots"}`)
			return
		}

		driverInst, ok := s.drivers.Get(req.AgentID)
		if !ok {
			http.Error(w, fmt.Sprintf("unsupported agent %q", req.AgentID), 400)
			return
		}
		if err := driverInst.Probe(); err != nil {
			http.Error(w, err.Error(), http.StatusServiceUnavailable)
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

		if err := s.bus.UpsertSession(sessionID, workspaceID, string(req.AgentID), targetCWD, req.CWD, worktreePath, branch, string(protocol.StatusRunning)); err != nil {
			if worktreePath != "" {
				_ = workspace.RemoveWorktree(req.CWD, worktreePath)
			}
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}

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

		drvSess, err := driverInst.Start(s.ctx, driver.SessionConfig{
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
			s.mu.Lock()
			st.Status = protocol.StatusStopped
			s.mu.Unlock()
			http.Error(w, err.Error(), http.StatusBadGateway)
			return
		}
		s.mu.Lock()
		st.DriverSess = drvSess
		status := st.Status
		s.mu.Unlock()

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(201)
		_ = json.NewEncoder(w).Encode(protocol.CreateSessionResponse{
			SessionID:    sessionID,
			WorkspaceID:  workspaceID,
			WorktreePath: strPtr(worktreePath),
			Status:       status,
		})

	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
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
	if len(parts) == 2 && (parts[1] == "input" || parts[1] == "resize") {
		if r.Method != http.MethodPost {
			methodNotAllowed(w, http.MethodPost)
			return
		}
		s.mu.RLock()
		state := s.sessions[id]
		var session driver.Session
		if state != nil {
			session = state.DriverSess
		}
		s.mu.RUnlock()
		if session == nil {
			http.Error(w, "session not found", http.StatusNotFound)
			return
		}
		var input struct {
			Data string `json:"data"`
			Cols int    `json:"cols"`
			Rows int    `json:"rows"`
		}
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1024*1024)).Decode(&input); err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		var err error
		if parts[1] == "input" {
			var data []byte
			data, err = base64.StdEncoding.DecodeString(input.Data)
			if err == nil {
				err = session.RawInput(data)
			}
		} else {
			err = session.Resize(input.Cols, input.Rows)
		}
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadRequest)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]bool{"ok": true})
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
		st, ok := s.snapshotSession(id)
		if !ok || st.DriverSess == nil {
			http.Error(w, `{"code":"ERR_SESSION_NOT_FOUND"}`, 404)
			return
		}
		if strings.TrimSpace(req.Prompt) == "" {
			http.Error(w, "prompt is empty", http.StatusBadRequest)
			return
		}
		message := protocol.ChatMessageEvent{BaseEvent: protocol.BaseEvent{SessionID: id, Timestamp: protocol.NowMillis()}, Type: protocol.EventChatMessage, MessageID: "user_" + workspace.NewID(), Role: protocol.RoleUser, Kind: "text", Text: req.Prompt, Rev: 1}
		s.broadcastEvent(id, string(message.Type), message)
		if err := st.DriverSess.Prompt(req.Prompt); err != nil {
			message.Kind, message.Rev, message.Text = "error", 2, req.Prompt+"\n\nNot delivered: "+err.Error()
			s.broadcastEvent(id, string(message.Type), message)
			http.Error(w, err.Error(), 500)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
		return
	}

	switch r.Method {
	case http.MethodDelete:
		st, ok := s.snapshotSession(id)

		if ok {
			if st.DriverSess != nil {
				_ = st.DriverSess.Close()
			}
			s.ptyManager.Kill(id)
			if st.WorktreePath != "" && st.OriginCWD != "" {
				if err := workspace.RemoveWorktree(st.OriginCWD, st.WorktreePath); err != nil {
					http.Error(w, "Session stopped; worktree retained: "+err.Error(), http.StatusConflict)
					return
				}
			}
		}
		if err := s.bus.DeleteSession(id); err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		s.mu.Lock()
		delete(s.sessions, id)
		s.mu.Unlock()
		if ok {
			st.Hub.Close()
		}
		s.pendingMu.Lock()
		for questionID, owner := range s.pendingQuestions {
			if owner == id {
				delete(s.pendingQuestions, questionID)
			}
		}
		s.pendingMu.Unlock()
		w.WriteHeader(204)

	case http.MethodGet:
		st, ok := s.snapshotSession(id)
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
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

// --- Approvals & Questions ---

// broadcastEvent persists an event to the bus and pushes it to all live
// WebSocket clients attached to the session's hub.
func (s *Server) broadcastEvent(sessionID, evType string, evt any) {
	s.mu.RLock()
	state := s.sessions[sessionID]
	s.mu.RUnlock()
	if state == nil {
		return
	}
	state.Hub.eventMu.Lock()
	defer state.Hub.eventMu.Unlock()
	seq, _ := s.bus.AppendEvent(sessionID, evType, evt)
	jb, _ := json.Marshal(evt)
	var withSeq map[string]any
	if err := json.Unmarshal(jb, &withSeq); err == nil {
		withSeq["seq"] = seq
		jb, _ = json.Marshal(withSeq)
	}
	st, ok := s.snapshotSession(sessionID)
	if ok && st.Hub != nil {
		st.Hub.Broadcast(protocol.Encode(protocol.OpcodeJSONRPC, 0, jb))
	}
}

func (s *Server) handleApproval(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		methodNotAllowed(w, http.MethodPost)
		return
	}
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

	app, err := s.approvals.ResolveWith(appID, req.Approved, "user", func(app *approval.PendingApproval) error {
		s.mu.RLock()
		st, ok := s.sessions[app.SessionID]
		var session driver.Session
		if ok {
			session = st.DriverSess
		}
		s.mu.RUnlock()
		if session == nil {
			return fmt.Errorf("session not running")
		}
		return session.Approve(appID, req.Approved)
	})
	if err != nil {
		http.Error(w, err.Error(), 404)
		return
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
	st, ok := s.snapshotSession(app.SessionID)

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
	if r.Method != http.MethodPost {
		methodNotAllowed(w, http.MethodPost)
		return
	}
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
	s.pendingMu.Lock()
	sessionID, known := s.pendingQuestions[qID]
	delete(s.pendingQuestions, qID)
	s.pendingMu.Unlock()
	if !known {
		http.Error(w, "question not found", http.StatusNotFound)
		return
	}

	delivered := false
	defer func() {
		if !delivered {
			s.pendingMu.Lock()
			s.pendingQuestions[qID] = sessionID
			s.pendingMu.Unlock()
		}
	}()
	if known {
		s.mu.RLock()
		st, ok := s.sessions[sessionID]
		var session driver.Session
		if ok {
			session = st.DriverSess
		}
		s.mu.RUnlock()
		if session != nil {
			// Forward a scalar for single-answer questions so drivers can
			// write the raw selection into the agent's stdin.
			var payload any = req.Answers
			if len(req.Answers) == 1 {
				payload = req.Answers[0]
			}
			if err := session.Answer(qID, payload); err != nil {
				http.Error(w, err.Error(), http.StatusBadGateway)
				return
			}
		} else {
			http.Error(w, "session not running", http.StatusConflict)
			return
		}
	}
	delivered = true

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
	if r.Method != http.MethodGet {
		methodNotAllowed(w, http.MethodGet)
		return
	}
	dir := r.URL.Query().Get("dir")
	if dir == "" {
		http.Error(w, "dir parameter required", 400)
		return
	}

	if !workspace.IsSafePathAny(s.cfg.AllowedRoots, dir) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusForbidden)
		_, _ = fmt.Fprint(w, `{"code":"ERR_PATH_TRAVERSAL"}`)
		return
	}

	entries, err := listWorkspaceFiles(s.cfg.AllowedRoots, dir)
	if err != nil {
		http.Error(w, err.Error(), 500)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(entries)
}

func (s *Server) handleDiff(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		methodNotAllowed(w, http.MethodGet)
		return
	}
	sessionID := strings.TrimPrefix(r.URL.Path, "/api/v1/diff/")
	st, ok := s.snapshotSession(sessionID)
	if !ok {
		http.Error(w, `{"code":"ERR_SESSION_NOT_FOUND"}`, 404)
		return
	}

	diff, err := workspaceDiff(r.Context(), st.CWD)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
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
			if err := s.tunnels.Stop(); err != nil {
				http.Error(w, err.Error(), http.StatusBadGateway)
				return
			}
			w.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
			return
		}
		u, err := s.tunnels.Start(s.ctx, req.Name, s.cfg.Addr)
		if err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true, "url": u})
	default:
		methodNotAllowed(w, http.MethodGet, http.MethodPost)
	}
}

func (s *Server) handleTelegramStatus(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		methodNotAllowed(w, http.MethodGet)
		return
	}
	st := s.telegram.Status()
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(st)
}

// --- WebSocket ---

func (s *Server) handleWS(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		methodNotAllowed(w, http.MethodGet)
		return
	}
	originPatterns := []string{}
	if origin := r.Header.Get("Origin"); origin != "" {
		if !isAllowedOrigin(origin, r.Host, s.cfg.AllowedOrigin) {
			http.Error(w, "origin not allowed", http.StatusForbidden)
			return
		}
		parsed, _ := url.Parse(origin)
		originPatterns = append(originPatterns, parsed.Host)
	}
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		OriginPatterns: originPatterns,
	})
	if err != nil {
		return
	}
	defer func() { _ = conn.Close(websocket.StatusNormalClosure, "") }()
	conn.SetReadLimit(1024 * 1024)

	sessionID := r.URL.Query().Get("sessionId")
	st, ok := s.snapshotSession(sessionID)

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
	eventsOnly := r.URL.Query().Get("eventsOnly") == "1"
	var lastDelivered int64
	if eventsOnly {
		lastDelivered, _ = strconv.ParseInt(r.URL.Query().Get("lastSeq"), 10, 64)
		if lastDelivered < 0 {
			lastDelivered = 0
		}
		if evs, err := s.bus.GetEventsSince(sessionID, lastDelivered); err == nil {
			for _, ev := range evs {
				payload, _ := json.Marshal(ev)
				if err := conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodeJSONRPC, 0, payload)); err != nil {
					return
				}
				if seq, ok := ev["seq"].(int64); ok {
					lastDelivered = seq
				}
			}
		}
	}

	// Writer goroutine
	go func() {
		for {
			select {
			case <-client.done:
				_ = conn.CloseNow()
				return
			case <-ctx.Done():
				return
			case frame, open := <-client.send:
				if !open {
					return
				}
				decoded, err := protocol.Decode(frame)
				if err != nil {
					continue
				}
				if eventsOnly && decoded.Opcode == protocol.OpcodePTYOutput {
					continue
				}
				if decoded.Opcode == protocol.OpcodeJSONRPC {
					var event struct {
						Type string `json:"type"`
						Seq  int64  `json:"seq"`
					}
					_ = json.Unmarshal(decoded.Payload, &event)
					if !eventsOnly && event.Type == string(protocol.EventStreamChunk) {
						continue
					}
					if eventsOnly && event.Seq > 0 {
						if event.Seq <= lastDelivered {
							continue
						}
						lastDelivered = event.Seq
					}
				}
				if err := conn.Write(ctx, websocket.MessageBinary, frame); err != nil {
					_ = conn.CloseNow()
					return
				}
			}
		}
	}()

	// Replay PTY ring buffer if session has active terminal
	if ok && !eventsOnly {
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
			if respBytes != nil {
				_ = conn.Write(ctx, websocket.MessageText, respBytes)
			}
			continue
		}

		frame, err := protocol.Decode(data)
		if err != nil {
			continue
		}

		switch frame.Opcode {
		case protocol.OpcodeKeystroke:
			st, ok := s.snapshotSession(sessionID)
			if ok && st.DriverSess != nil {
				if term, isTerm := st.DriverSess.(driver.Terminal); isTerm {
					_ = term.RawInput(frame.Payload)
				}
			}

		case protocol.OpcodeViewportResize:
			if cols, rows, err := protocol.DecodeResize(frame.Payload); err == nil {
				st, ok := s.snapshotSession(sessionID)
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
			if respBytes != nil {
				_ = conn.Write(ctx, websocket.MessageBinary, protocol.Encode(protocol.OpcodeJSONRPC, frame.Slot, respBytes))
			}
		}
	}
}

// --- SSE ---

// --- RPC Handlers ---

// --- serverSink implements driver.Sink ---

type serverSink struct {
	server    *Server
	sessionID string
	hub       *Hub
	parser    *parser.StreamParser
	mu        sync.Mutex
	lineBuf   string // incomplete line carried across Bytes calls
}

func (s *serverSink) Bytes(data []byte) {
	s.Event(protocol.StreamChunkEvent{BaseEvent: protocol.BaseEvent{SessionID: s.sessionID, Timestamp: protocol.NowMillis()}, Type: protocol.EventStreamChunk, Chunk: base64.StdEncoding.EncodeToString(data), Encoding: "base64"})
	frame := protocol.Encode(protocol.OpcodePTYOutput, 0, data)
	s.hub.Broadcast(frame)

	// Feed the parser only on complete, ANSI-stripped lines.
	// Raw PTY chunks are arbitrary byte boundaries — never feed them directly.
	s.mu.Lock()
	s.lineBuf += stripANSI(string(data))
	if len(s.lineBuf) > 64*1024 {
		s.lineBuf = s.lineBuf[len(s.lineBuf)-64*1024:]
	}
	var completeLines []string
	for {
		idx := strings.Index(s.lineBuf, "\n")
		if idx < 0 {
			break
		}
		if line := strings.TrimRight(s.lineBuf[:idx], "\r"); line != "" {
			completeLines = append(completeLines, line)
		}
		s.lineBuf = s.lineBuf[idx+1:]
	}
	s.mu.Unlock()

	for _, line := range completeLines {
		for _, ev := range s.parser.FeedLine(line) {
			s.Event(ev)
		}
	}
}
func (s *serverSink) Message(msg chat.Message) {
	if msg.Role == protocol.RoleUser {
		return
	} // prompts are recorded once by the shared REST/RPC handler
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

	s.Event(evt)

	s.server.telegram.NotifyChatMessage(s.server.ctx, s.server.cfg.TelegramChatID, msg)
}

func (s *serverSink) Event(evt any) {
	s.hub.eventMu.Lock()
	defer s.hub.eventMu.Unlock()
	switch event := evt.(type) {
	case protocol.ApprovalRequestedEvent:
		s.server.approvals.Put(&approval.PendingApproval{ID: event.ApprovalID, SessionID: s.sessionID, ToolName: event.ToolName, Command: event.Command, AutoDenyTimeoutMs: event.AutoDenyTimeoutMs})
		if app, ok := s.server.approvals.Get(event.ApprovalID); ok {
			s.server.telegram.NotifyApproval(s.server.ctx, s.server.cfg.TelegramChatID, app)
		}
	case protocol.QuestionAskedEvent:
		s.server.pendingMu.Lock()
		s.server.pendingQuestions[event.QuestionID] = s.sessionID
		s.server.pendingMu.Unlock()
		s.server.telegram.NotifyQuestion(s.server.ctx, s.server.cfg.TelegramChatID, event)
	case protocol.ArtifactUpdatedEvent:
		s.server.telegram.NotifyArtifact(s.server.ctx, s.server.cfg.TelegramChatID, event)
	case protocol.DiffGeneratedEvent:
		s.server.telegram.NotifyArtifact(s.server.ctx, s.server.cfg.TelegramChatID, protocol.ArtifactUpdatedEvent{BaseEvent: event.BaseEvent, Type: protocol.EventArtifactUpdated, Path: event.FilePath + ".patch", Kind: "diff", Content: event.DiffPatch})
	}
	var evType string
	if b, ok := evt.(protocol.AgentEvent); ok {
		evType = string(b.Type)
	} else if chunk, ok := evt.(protocol.StreamChunkEvent); ok {
		evType = string(chunk.Type)
	} else if message, ok := evt.(protocol.ChatMessageEvent); ok {
		evType = string(message.Type)
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
	} else if artifact, ok := evt.(protocol.ArtifactUpdatedEvent); ok {
		evType = string(artifact.Type)
	} else if status, ok := evt.(protocol.SessionStatusEvent); ok {
		evType = string(status.Type)
	} else {
		evType = "custom.event"
	}

	seq, err := s.server.bus.AppendEvent(s.sessionID, evType, evt)
	if err != nil {
		log.Printf("[events] persist %s for %s: %v", evType, s.sessionID, err)
		return
	}
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

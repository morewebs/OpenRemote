package server

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"strings"

	"github.com/morewebs/OpenRemote/internal/core/rpc"
)

// REST and RPC use the same operations, including validation and event delivery.
func (s *Server) setupRPC() {
	register := func(names []string, method, route, key string, handler http.HandlerFunc) {
		for _, name := range names {
			s.rpcMux.Register(name, func(ctx context.Context, sessionID string, params json.RawMessage) (any, *rpc.RPCError) {
				var args map[string]json.RawMessage
				if len(params) > 0 {
					if err := json.Unmarshal(params, &args); err != nil {
						return nil, &rpc.RPCError{Code: rpc.ErrInvalidParams, Message: err.Error()}
					}
				}
				path := route
				if key != "" {
					id := ""
					if key == "sessionId" {
						id = sessionID
					}
					if raw, ok := args[key]; ok {
						if err := json.Unmarshal(raw, &id); err != nil {
							return nil, &rpc.RPCError{Code: rpc.ErrInvalidParams, Message: key + " must be a string"}
						}
					}
					if id == "" || strings.ContainsAny(id, "/\\?#") {
						return nil, &rpc.RPCError{Code: rpc.ErrInvalidParams, Message: key + " required"}
					}
					path = strings.ReplaceAll(path, "{id}", url.PathEscape(id))
				}
				req, err := http.NewRequestWithContext(ctx, method, path, bytes.NewReader(params))
				if err != nil {
					return nil, &rpc.RPCError{Code: rpc.ErrInternalError, Message: err.Error()}
				}
				response := &rpcHTTPResponse{header: make(http.Header), status: http.StatusOK}
				handler(response, req)
				if response.status >= 400 {
					code := rpc.ErrInvalidParams
					if response.status >= 500 {
						code = rpc.ErrInternalError
					}
					return nil, &rpc.RPCError{Code: code, Message: strings.TrimSpace(response.body.String()), Data: map[string]any{"httpStatus": response.status}}
				}
				if response.body.Len() == 0 {
					return map[string]any{"ok": true}, nil
				}
				var result any
				if err := json.Unmarshal(response.body.Bytes(), &result); err != nil {
					return nil, &rpc.RPCError{Code: rpc.ErrInternalError, Message: err.Error()}
				}
				return result, nil
			})
		}
	}
	register([]string{"session.create"}, http.MethodPost, "/api/v1/sessions", "", s.handleSessions)
	register([]string{"session.list"}, http.MethodGet, "/api/v1/sessions", "", s.handleSessions)
	register([]string{"session.stop"}, http.MethodDelete, "/api/v1/sessions/{id}", "sessionId", s.handleSessionByID)
	register([]string{"session.sendPrompt", "prompt.send"}, http.MethodPost, "/api/v1/sessions/{id}/prompt", "sessionId", s.handleSessionByID)
	register([]string{"session.approve", "approval.resolve"}, http.MethodPost, "/api/v1/approval/{id}", "approvalId", s.handleApproval)
	register([]string{"session.answer"}, http.MethodPost, "/api/v1/question/{id}", "questionId", s.handleQuestion)
	register([]string{"system.status"}, http.MethodGet, "/health", "", s.handleHealth)
	register([]string{"system.agents", "agents.list"}, http.MethodGet, "/api/v1/agents", "", s.handleAgents)
	register([]string{"system.tunnels"}, http.MethodGet, "/api/v1/tunnels", "", s.handleTunnels)
	s.rpcMux.Register("session.resize", func(ctx context.Context, sessionID string, params json.RawMessage) (any, *rpc.RPCError) {
		var req struct {
			SessionID string `json:"sessionId"`
			Cols      int    `json:"cols"`
			Rows      int    `json:"rows"`
		}
		if err := json.Unmarshal(params, &req); err != nil {
			return nil, &rpc.RPCError{Code: rpc.ErrInvalidParams, Message: err.Error()}
		}
		if req.SessionID != "" {
			sessionID = req.SessionID
		}
		s.mu.RLock()
		st, ok := s.sessions[sessionID]
		if !ok || st.DriverSess == nil {
			s.mu.RUnlock()
			return nil, &rpc.RPCError{Code: rpc.ErrInvalidParams, Message: "session not found"}
		}
		driver := st.DriverSess
		s.mu.RUnlock()
		if err := driver.Resize(req.Cols, req.Rows); err != nil {
			return nil, &rpc.RPCError{Code: rpc.ErrInternalError, Message: err.Error()}
		}
		return map[string]any{"ok": true}, nil
	})
}

type rpcHTTPResponse struct {
	header http.Header
	status int
	body   bytes.Buffer
}

func (r *rpcHTTPResponse) Header() http.Header            { return r.header }
func (r *rpcHTTPResponse) WriteHeader(status int)         { r.status = status }
func (r *rpcHTTPResponse) Write(data []byte) (int, error) { return r.body.Write(data) }

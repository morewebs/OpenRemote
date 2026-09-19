package server

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"

	"github.com/morewebs/OpenRemote/internal/telegram"
)

func (s *Server) telegramHandlers() telegram.Handlers {
	invoke := func(ctx context.Context, method, path string, body any, handler http.HandlerFunc) ([]byte, error) {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		req, err := http.NewRequestWithContext(ctx, method, path, bytes.NewReader(data))
		if err != nil {
			return nil, err
		}
		response := &rpcHTTPResponse{header: make(http.Header), status: http.StatusOK}
		handler(response, req)
		if response.status >= 400 {
			return nil, fmt.Errorf("%s", response.body.String())
		}
		return response.body.Bytes(), nil
	}
	return telegram.Handlers{
		Prompt: func(ctx context.Context, id, text string) error {
			_, err := invoke(ctx, http.MethodPost, "/api/v1/sessions/"+url.PathEscape(id)+"/prompt", map[string]string{"prompt": text}, s.handleSessionByID)
			return err
		},
		Approval: func(ctx context.Context, id string, approved bool) error {
			_, err := invoke(ctx, http.MethodPost, "/api/v1/approval/"+url.PathEscape(id), map[string]bool{"approved": approved}, s.handleApproval)
			return err
		},
		Answer: func(ctx context.Context, id string, answers []any) error {
			_, err := invoke(ctx, http.MethodPost, "/api/v1/question/"+url.PathEscape(id), map[string]any{"answers": answers}, s.handleQuestion)
			return err
		},
		Stop: func(ctx context.Context, id string) error {
			_, err := invoke(ctx, http.MethodDelete, "/api/v1/sessions/"+url.PathEscape(id), nil, s.handleSessionByID)
			return err
		},
		Create: func(ctx context.Context, agent, cwd string) (string, error) {
			data, err := invoke(ctx, http.MethodPost, "/api/v1/sessions", map[string]any{"agentId": agent, "cwd": cwd}, s.handleSessions)
			if err != nil {
				return "", err
			}
			var result struct {
				SessionID string `json:"sessionId"`
			}
			err = json.Unmarshal(data, &result)
			return result.SessionID, err
		},
	}
}

package rpc

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
)

type Request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      any             `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

type Response struct {
	JSONRPC string    `json:"jsonrpc"`
	ID      any       `json:"id,omitempty"`
	Result  any       `json:"result,omitempty"`
	Error   *RPCError `json:"error,omitempty"`
}

type RPCError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
	Data    any    `json:"data,omitempty"`
}

func (e *RPCError) Error() string {
	return fmt.Sprintf("RPC error %d: %s", e.Code, e.Message)
}

const (
	ErrParseError     = -32700
	ErrInvalidRequest = -32600
	ErrMethodNotFound = -32601
	ErrInvalidParams  = -32602
	ErrInternalError  = -32603
)

type Handler func(ctx context.Context, sessionID string, params json.RawMessage) (any, *RPCError)

type Mux struct {
	mu       sync.RWMutex
	handlers map[string]Handler
}

func NewMux() *Mux {
	return &Mux{
		handlers: make(map[string]Handler),
	}
}

func (m *Mux) Register(method string, handler Handler) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.handlers[method] = handler
}

func (m *Mux) Dispatch(ctx context.Context, sessionID string, data []byte) ([]byte, error) {
	var req Request
	if err := json.Unmarshal(data, &req); err != nil {
		resp := Response{
			JSONRPC: "2.0",
			Error:   &RPCError{Code: ErrParseError, Message: "Parse error: " + err.Error()},
		}
		return json.Marshal(resp)
	}

	m.mu.RLock()
	handler, ok := m.handlers[req.Method]
	m.mu.RUnlock()

	if !ok {
		resp := Response{
			JSONRPC: "2.0",
			ID:      req.ID,
			Error:   &RPCError{Code: ErrMethodNotFound, Message: fmt.Sprintf("Method %q not found", req.Method)},
		}
		return json.Marshal(resp)
	}

	res, rpcErr := handler(ctx, sessionID, req.Params)
	resp := Response{
		JSONRPC: "2.0",
		ID:      req.ID,
		Result:  res,
		Error:   rpcErr,
	}
	return json.Marshal(resp)
}

package rpc

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"sync"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

type Request = protocol.RPCRequest
type Response = protocol.RPCResponse
type RPCError = protocol.RPCError

const (
	ErrParseError     = -32700
	ErrInvalidRequest = -32600
	ErrMethodNotFound = -32601
	ErrInvalidParams  = -32602
	ErrInternalError  = -32603
)

type Handler func(context.Context, string, json.RawMessage) (any, *RPCError)
type Mux struct {
	mu       sync.RWMutex
	handlers map[string]Handler
}

func NewMux() *Mux { return &Mux{handlers: make(map[string]Handler)} }
func (m *Mux) Register(method string, handler Handler) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.handlers[method] = handler
}
func failure(id any, code int, message string) ([]byte, error) {
	return json.Marshal(Response{JSONRPC: "2.0", ID: id, Error: &RPCError{Code: code, Message: message}})
}

// Dispatch implements JSON-RPC 2.0 requests, notifications, and batches.
// A nil response means the caller must not send a frame (notification only).
func (m *Mux) Dispatch(ctx context.Context, sessionID string, data []byte) ([]byte, error) {
	data = bytes.TrimSpace(data)
	if !json.Valid(data) {
		return failure(nil, ErrParseError, "Parse error")
	}
	if len(data) > 0 && data[0] == '[' {
		var requests []json.RawMessage
		_ = json.Unmarshal(data, &requests)
		if len(requests) == 0 {
			return failure(nil, ErrInvalidRequest, "Empty batch")
		}
		responses := make([]json.RawMessage, 0, len(requests))
		for _, request := range requests {
			response, err := m.dispatchOne(ctx, sessionID, request)
			if err != nil {
				return nil, err
			}
			if response != nil {
				responses = append(responses, response)
			}
		}
		if len(responses) == 0 {
			return nil, nil
		}
		return json.Marshal(responses)
	}
	return m.dispatchOne(ctx, sessionID, data)
}
func (m *Mux) dispatchOne(ctx context.Context, sessionID string, data []byte) ([]byte, error) {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil || fields == nil {
		return failure(nil, ErrInvalidRequest, "Invalid request")
	}
	var req Request
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()
	if err := dec.Decode(&req); err != nil {
		return failure(nil, ErrInvalidRequest, "Invalid request")
	}
	switch req.ID.(type) {
	case nil, string, json.Number:
	default:
		return failure(nil, ErrInvalidRequest, "Invalid request id")
	}
	if req.JSONRPC != "2.0" || req.Method == "" {
		return failure(req.ID, ErrInvalidRequest, "Expected JSON-RPC 2.0 method")
	}
	_, hasID := fields["id"]
	respond := func(result any, rpcErr *RPCError) ([]byte, error) {
		if !hasID {
			return nil, nil
		}
		return json.Marshal(Response{JSONRPC: "2.0", ID: req.ID, Result: result, Error: rpcErr})
	}
	if p, ok := fields["params"]; ok {
		p = bytes.TrimSpace(p)
		if len(p) == 0 || (p[0] != '{' && p[0] != '[') {
			return respond(nil, &RPCError{Code: ErrInvalidParams, Message: "Params must be an object or array"})
		}
	}
	m.mu.RLock()
	handler, ok := m.handlers[req.Method]
	m.mu.RUnlock()
	if !ok {
		return respond(nil, &RPCError{Code: ErrMethodNotFound, Message: fmt.Sprintf("Method %q not found", req.Method)})
	}
	result, rpcErr := handler(ctx, sessionID, req.Params)
	return respond(result, rpcErr)
}

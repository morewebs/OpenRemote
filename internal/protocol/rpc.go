package protocol

import (
	"encoding/json"
	"fmt"
)

// JSON-RPC envelope used over 0x05 frames (WS mux) and plain HTTP JSON.
// Kept tiny — zod-to-go translation of shared-protocol/rpc/index.ts.
//
// Status note: the wire envelope actually dispatched by the daemon lives in
// internal/core/rpc (JSON-RPC 2.0 with integer error codes and `any` ids,
// registered per-method on rpc.Mux by the server). The types below are the
// legacy/typed view of the same protocol; RPCRequest/RPCResponse are not on
// the live dispatch path. The well-known string codes (ErrAuthRequired etc.)
// and payload types (FileEntry, ApprovalRequest) ARE used by server helpers.
//
// Spec 04 §2 core methods (session.create, session.list, session.stop,
// session.sendPrompt, session.approve, session.answer, session.resize,
// system.status, system.agents, system.tunnels) are registered on rpc.Mux in
// internal/core/server — wiring owned there, not here.

type RPCRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      any             `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

type RPCResponse struct {
	JSONRPC string    `json:"jsonrpc"`
	ID      any       `json:"id"`
	Result  any       `json:"result,omitempty"`
	Error   *RPCError `json:"error,omitempty"`
}

type RPCError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
	Data    any    `json:"data,omitempty"`
}

func (e RPCError) Error() string { return fmt.Sprintf("RPC error %d: %s", e.Code, e.Message) }

func (r RPCResponse) MarshalJSON() ([]byte, error) {
	value := map[string]any{"jsonrpc": "2.0", "id": r.ID}
	if r.Error != nil {
		value["error"] = r.Error
	} else {
		value["result"] = r.Result
	}
	return json.Marshal(value)
}

// Well-known RPC codes — spec 04 Error Codes Table
const (
	ErrAuthRequired    = "ERR_AUTH_REQUIRED"
	ErrPathTraversal   = "ERR_PATH_TRAVERSAL"
	ErrSessionNotFound = "ERR_SESSION_NOT_FOUND"
	ErrConPTYException = "ERR_CONPTY_EXCEPTION"
	ErrRateLimited     = "ERR_RATE_LIMITED"
)

type ApprovalRequest struct {
	ApprovalID string `json:"approvalId"`
	Approved   bool   `json:"approved"`
}

type FileListRequest struct {
	Dir string `json:"dir"`
}

type FileEntry struct {
	Name  string `json:"name"`
	Path  string `json:"path"`
	IsDir bool   `json:"isDir"`
	Size  int64  `json:"size"`
}

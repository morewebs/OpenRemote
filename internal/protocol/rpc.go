package protocol

import "encoding/json"

// JSON-RPC envelope used over 0x05 frames (WS mux) and plain HTTP JSON.
// Kept tiny — zod-to-go translation of shared-protocol/rpc/index.ts.

type RPCRequest struct {
	ID     string          `json:"id"`
	Method string          `json:"method"`
	Params json.RawMessage `json:"params,omitempty"`
}

type RPCResponse struct {
	ID     string          `json:"id"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  *RPCError       `json:"error,omitempty"`
}

type RPCError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func (e RPCError) Error() string { return e.Code + ": " + e.Message }

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
	IsDir bool   `json:"isDir"`
	Size  int64  `json:"size"`
}

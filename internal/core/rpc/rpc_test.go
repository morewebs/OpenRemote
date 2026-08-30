package rpc

import (
	"context"
	"encoding/json"
	"testing"
)

func TestMux_Dispatch(t *testing.T) {
	mux := NewMux()
	mux.Register("ping", func(ctx context.Context, sessionID string, params json.RawMessage) (any, *RPCError) {
		return map[string]string{"reply": "pong", "session": sessionID}, nil
	})
	mux.Register("error_test", func(ctx context.Context, sessionID string, params json.RawMessage) (any, *RPCError) {
		return nil, &RPCError{Code: ErrInvalidParams, Message: "invalid parameter"}
	})

	ctx := context.Background()

	// 1. Success dispatch
	reqSuccess := `{"jsonrpc":"2.0","id":"123","method":"ping","params":{}}`
	resBytes, err := mux.Dispatch(ctx, "ses_test", []byte(reqSuccess))
	if err != nil {
		t.Fatalf("unexpected dispatch error: %v", err)
	}

	var res Response
	if err := json.Unmarshal(resBytes, &res); err != nil {
		t.Fatalf("failed to parse response: %v", err)
	}
	if res.Error != nil {
		t.Fatalf("unexpected error in response: %v", res.Error)
	}
	resMap, ok := res.Result.(map[string]any)
	if !ok || resMap["reply"] != "pong" || resMap["session"] != "ses_test" {
		t.Fatalf("unexpected result: %+v", res.Result)
	}

	// 2. Error response
	reqErr := `{"jsonrpc":"2.0","id":"456","method":"error_test"}`
	resBytes, _ = mux.Dispatch(ctx, "ses_test", []byte(reqErr))
	var res2 Response
	_ = json.Unmarshal(resBytes, &res2)
	if res2.Error == nil || res2.Error.Code != ErrInvalidParams {
		t.Fatalf("expected ErrInvalidParams, got %+v", res2.Error)
	}

	// 3. Method not found
	reqNotFound := `{"jsonrpc":"2.0","id":"789","method":"nonexistent"}`
	resBytes, _ = mux.Dispatch(ctx, "ses_test", []byte(reqNotFound))
	var res3 Response
	_ = json.Unmarshal(resBytes, &res3)
	if res3.Error == nil || res3.Error.Code != ErrMethodNotFound {
		t.Fatalf("expected ErrMethodNotFound, got %+v", res3.Error)
	}

	// 4. Invalid JSON
	resBytes, _ = mux.Dispatch(ctx, "ses_test", []byte(`{invalid-json`))
	var res4 Response
	_ = json.Unmarshal(resBytes, &res4)
	if res4.Error == nil || res4.Error.Code != ErrParseError {
		t.Fatalf("expected ErrParseError, got %+v", res4.Error)
	}
}

package protocol_test

import (
	"testing"

	"github.com/morewebs/OpenRemote/internal/protocol"
)

func TestBinaryFraming(t *testing.T) {
	payload := []byte("hello world ansi terminal data \x1b[31mred\x1b[0m")
	slot := byte(2)
	frame := protocol.Encode(protocol.OpcodePTYOutput, slot, payload)

	decoded, err := protocol.Decode(frame)
	if err != nil {
		t.Fatalf("Decode failed: %v", err)
	}

	if decoded.Opcode != protocol.OpcodePTYOutput {
		t.Errorf("expected OpcodePTYOutput, got %v", decoded.Opcode)
	}
	if decoded.Slot != slot {
		t.Errorf("expected slot %d, got %d", slot, decoded.Slot)
	}
	if string(decoded.Payload) != string(payload) {
		t.Errorf("payload mismatch: got %q, want %q", string(decoded.Payload), string(payload))
	}
}

func TestResizeFraming(t *testing.T) {
	cols, rows := uint16(140), uint16(45)
	payload := protocol.EncodeResize(cols, rows)

	dCols, dRows, err := protocol.DecodeResize(payload)
	if err != nil {
		t.Fatalf("DecodeResize failed: %v", err)
	}
	if dCols != cols || dRows != rows {
		t.Errorf("got (%d, %d), want (%d, %d)", dCols, dRows, cols, rows)
	}
}

func TestCatchupFraming(t *testing.T) {
	lastSeq := uint32(98214)
	payload := protocol.EncodeCatchup(lastSeq)

	dSeq, err := protocol.DecodeCatchup(payload)
	if err != nil {
		t.Fatalf("DecodeCatchup failed: %v", err)
	}
	if dSeq != lastSeq {
		t.Errorf("got seq %d, want %d", dSeq, lastSeq)
	}
}

func TestPingPongFraming(t *testing.T) {
	ts := uint64(1755861234567)
	payload := protocol.EncodePing(ts)

	dTS, err := protocol.DecodePing(payload)
	if err != nil {
		t.Fatalf("DecodePing failed: %v", err)
	}
	if dTS != ts {
		t.Errorf("got ts %d, want %d", dTS, ts)
	}
}

func TestEventUnmarshal(t *testing.T) {
	rawAppr := `{
		"type": "approval.requested",
		"seq": 42,
		"sessionId": "ses_123",
		"timestamp": 1700000000,
		"approvalId": "app_99",
		"toolName": "bash",
		"command": "git push origin main",
		"autoDenyTimeoutMs": 300000
	}`

	ev, err := protocol.UnmarshalEvent([]byte(rawAppr))
	if err != nil {
		t.Fatalf("UnmarshalEvent failed: %v", err)
	}

	appr, ok := ev.(protocol.ApprovalRequestedEvent)
	if !ok {
		t.Fatalf("expected ApprovalRequestedEvent, got %T", ev)
	}

	if appr.ApprovalID != "app_99" || appr.ToolName != "bash" || appr.Seq != 42 {
		t.Errorf("unexpected event content: %+v", appr)
	}
}

func TestSessionValidation(t *testing.T) {
	req := protocol.CreateSessionRequest{
		AgentID:     protocol.AgentClaude,
		CWD:         "/test/dir",
		UseWorktree: true,
	}
	if err := req.Validate(); err != nil {
		t.Fatalf("validation failed: %v", err)
	}
	if req.Cols != 120 || req.Rows != 30 {
		t.Errorf("expected default (120, 30), got (%d, %d)", req.Cols, req.Rows)
	}

	bad := protocol.CreateSessionRequest{
		AgentID: "unknown-agent",
		CWD:     "",
	}
	if err := bad.Validate(); err == nil {
		t.Error("expected error for invalid agent and empty cwd")
	}
}

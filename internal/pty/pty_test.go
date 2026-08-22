package pty_test

import (
	"context"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestSlidingRingBuffer(t *testing.T) {
	buf := pty.NewSlidingRingBuffer(10)

	buf.Push([]byte("12345"))
	if string(buf.ReadAll()) != "12345" {
		t.Errorf("got %q, want '12345'", string(buf.ReadAll()))
	}

	buf.Push([]byte("67890"))
	if string(buf.ReadAll()) != "1234567890" {
		t.Errorf("got %q, want '1234567890'", string(buf.ReadAll()))
	}

	// Overflow with wrap-around
	buf.Push([]byte("ABC"))
	if string(buf.ReadAll()) != "4567890ABC" {
		t.Errorf("got %q, want '4567890ABC'", string(buf.ReadAll()))
	}

	// Massive chunk exceeding entire capacity
	buf.Push([]byte("XYZ1234567890EXTRA"))
	if string(buf.ReadAll()) != "4567890EXTRA"[len("4567890EXTRA")-10:] {
		t.Errorf("got %q", string(buf.ReadAll()))
	}
}

func TestClampDimensions(t *testing.T) {
	c, r := pty.ClampDimensions(5, 2)
	if c != 20 || r != 5 {
		t.Errorf("clamp min failed: got (%d, %d)", c, r)
	}

	c, r = pty.ClampDimensions(500, 200)
	if c != 300 || r != 100 {
		t.Errorf("clamp max failed: got (%d, %d)", c, r)
	}
}

func TestPTYSpawnAndEcho(t *testing.T) {
	mgr := pty.NewManager()
	dataCh := make(chan []byte, 10)

	cfg := pty.SpawnConfig{
		SessionID: "test-echo",
		Command:   "cmd.exe",
		Args:      []string{"/c", "echo openremote-test-echo"},
	}

	inst, err := mgr.Spawn(context.Background(), cfg)
	if err != nil {
		t.Skipf("skipping spawn on non-windows / missing cmd: %v", err)
	}
	inst.OnData = func(chunk []byte) {
		dataCh <- chunk
	}

	select {
	case chunk := <-dataCh:
		if len(chunk) == 0 {
			t.Error("received empty chunk")
		}
	case <-time.After(3 * time.Second):
		t.Error("timed out waiting for echo output")
	}

	mgr.Kill("test-echo")
}

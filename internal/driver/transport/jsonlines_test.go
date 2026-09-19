package transport

import (
	"context"
	"encoding/json"
	"os"
	"sync"
	"testing"
	"time"
)

func TestJSONLinesHelper(t *testing.T) {
	if os.Args[len(os.Args)-1] != "--jsonlines-helper" {
		return
	}
	decoder, encoder := json.NewDecoder(os.Stdin), json.NewEncoder(os.Stdout)
	for {
		var msg Message
		if decoder.Decode(&msg) != nil {
			os.Exit(0)
		}
		_ = encoder.Encode(map[string]any{"method": "progress", "params": map[string]any{"ok": true}})
		if msg.Method != "wait" {
			_ = encoder.Encode(map[string]any{"id": msg.ID, "result": map[string]any{"echo": msg.Method}})
		}
	}
}

func TestConcurrentRequestsNotificationsAndCancellation(t *testing.T) {
	bin, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	events := make(chan Message, 30)
	client, err := Start(ctx, bin, []string{"-test.run=^TestJSONLinesHelper$", "--", "--jsonlines-helper"}, "", nil, func(msg Message) { events <- msg }, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	var wg sync.WaitGroup
	for range 10 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			var result struct{ Echo string }
			if err := client.Call(ctx, "echo", map[string]any{}, &result); err != nil || result.Echo != "echo" {
				t.Errorf("call = %+v, %v", result, err)
			}
		}()
	}
	wg.Wait()
	if len(events) != 10 {
		t.Fatalf("received %d notifications", len(events))
	}
	short, stop := context.WithTimeout(ctx, 25*time.Millisecond)
	defer stop()
	if err := client.Call(short, "wait", nil, nil); err != context.DeadlineExceeded {
		t.Fatalf("cancellation = %v", err)
	}
	client.mu.Lock()
	pending := len(client.pending)
	client.mu.Unlock()
	if pending != 0 {
		t.Fatalf("leaked %d pending calls", pending)
	}
	if err := client.Close(); err != nil {
		t.Fatal(err)
	}
	if err := client.Call(ctx, "after-close", nil, nil); err == nil {
		t.Fatal("call after exit succeeded")
	}
}

func TestCloseWaitsForExitDelivery(t *testing.T) {
	bin, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	entered, release := make(chan struct{}), make(chan struct{})
	client, err := Start(context.Background(), bin, []string{"-test.run=^TestJSONLinesHelper$", "--", "--jsonlines-helper"}, "", nil, nil, nil, func(int) { close(entered); <-release })
	if err != nil {
		t.Fatal(err)
	}
	closed := make(chan struct{})
	go func() { _ = client.Close(); close(closed) }()
	select {
	case <-entered:
	case <-time.After(5 * time.Second):
		close(release)
		t.Fatal("exit callback not delivered")
	}
	select {
	case <-closed:
		t.Error("Close returned before exit callback finished")
	default:
	}
	close(release)
	select {
	case <-closed:
	case <-time.After(time.Second):
		t.Fatal("Close did not finish")
	}
}

package tunnel

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"testing"
	"time"
)

func TestTunnelHelper(t *testing.T) {
	mode := os.Args[len(os.Args)-1]
	if mode != "--tunnel-ready" && mode != "--tunnel-wait" {
		return
	}
	if mode == "--tunnel-ready" {
		fmt.Println("https://openremote-test.trycloudflare.com")
	}
	<-time.After(time.Hour)
	os.Exit(0)
}
func fakeTunnel(t *testing.T, mode string) *processProvider {
	t.Helper()
	bin, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	p := NewCloudflaredProvider().processProvider
	p.name = bin
	p.command = func(ctx context.Context, _ string, _ ...string) *exec.Cmd {
		return exec.CommandContext(ctx, bin, "-test.run=^TestTunnelHelper$", "--", mode)
	}
	t.Cleanup(func() { _ = p.Stop() })
	return p
}
func TestTunnelLifecycleAndTimeout(t *testing.T) {
	p := fakeTunnel(t, "--tunnel-ready")
	url, err := p.Start(context.Background(), "127.0.0.1:4097")
	if err != nil || url != "https://openremote-test.trycloudflare.com" {
		t.Fatalf("start=%s, %v", url, err)
	}
	if err := p.Stop(); err != nil {
		t.Fatal(err)
	}
	if p.Info().Running {
		t.Fatal("stopped tunnel reported running")
	}
	waiting := fakeTunnel(t, "--tunnel-wait")
	waiting.timeout = 100 * time.Millisecond
	if _, err := waiting.Start(context.Background(), "127.0.0.1:4097"); err == nil {
		t.Fatal("startup did not time out")
	}
	if waiting.Info().Running {
		t.Fatal("timed out process retained")
	}
}

func TestStopCancelsPreflight(t *testing.T) {
	p := fakeTunnel(t, "--tunnel-ready")
	entered := make(chan struct{})
	p.preflight = func(ctx context.Context, _ string) error {
		close(entered)
		<-ctx.Done()
		return ctx.Err()
	}
	result := make(chan error, 1)
	go func() { _, err := p.Start(context.Background(), "127.0.0.1:4097"); result <- err }()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("preflight not started")
	}
	if err := p.Stop(); err != nil {
		t.Fatal(err)
	}
	if err := <-result; err == nil {
		t.Fatal("cancelled startup succeeded")
	}
	if p.Info().Running {
		t.Fatal("process started after stop")
	}
}

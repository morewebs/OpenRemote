package pty

import (
	"context"
	"encoding/json"
	"os"
	"runtime"
	"strings"
	"testing"
	"time"
)

func TestPTYWorkerHelper(t *testing.T) {
	mode := os.Args[len(os.Args)-1]
	if mode == "--pty-worker" {
		if err := NewWorker().Run(context.Background()); err != nil {
			os.Exit(1)
		}
		os.Exit(0)
	}
	if mode == "--crashing-worker" {
		var msg IPCMessage
		_ = json.NewDecoder(os.Stdin).Decode(&msg)
		_ = json.NewEncoder(os.Stdout).Encode(IPCMessage{Type: MsgSpawned, SessionID: msg.SessionID})
		os.Exit(42)
	}
}

func isolatedTestManager(t *testing.T, mode string) *Manager {
	t.Helper()
	bin, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	mgr := NewIsolatedManager(bin, "-test.run=^TestPTYWorkerHelper$", "--", mode)
	t.Cleanup(mgr.Close)
	return mgr
}

func TestIsolatedWorkerRoundTrip(t *testing.T) {
	mgr := isolatedTestManager(t, "--pty-worker")
	cfg := SpawnConfig{SessionID: "isolated-echo", Command: "/bin/sh", Cols: 80, Rows: 24}
	if runtime.GOOS == "windows" {
		cfg.Command, cfg.Args = "cmd.exe", []string{"/Q"}
	}
	exit := make(chan int, 1)
	inst, err := mgr.Spawn(context.Background(), cfg, Hooks{OnExit: func(code int, _ string) { exit <- code }})
	if err != nil {
		t.Fatal(err)
	}
	inst.Resize(100, 40)
	if err := inst.Write([]byte("echo isolated-worker-output\r\nexit\r\n")); err != nil {
		t.Fatal(err)
	}
	select {
	case code := <-exit:
		if code != 0 {
			t.Errorf("exit code = %d", code)
		}
	case <-time.After(15 * time.Second):
		t.Fatal("worker did not exit")
	}
	if !strings.Contains(string(inst.RingBuffer.ReadAll()), "isolated-worker-output") {
		t.Fatal("terminal output was lost")
	}
	if mgr.Count() != 0 {
		t.Fatal("exited PTY retained by manager")
	}
	select {
	case <-inst.remote.done:
	case <-time.After(5 * time.Second):
		t.Fatal("worker process leaked")
	}
}

func TestIsolatedWorkerCrash(t *testing.T) {
	mgr := isolatedTestManager(t, "--crashing-worker")
	exit := make(chan int, 1)
	_, err := mgr.Spawn(context.Background(), SpawnConfig{SessionID: "crash"}, Hooks{OnExit: func(code int, _ string) { exit <- code }})
	if err != nil {
		t.Fatal(err)
	}
	select {
	case code := <-exit:
		if code != -1 {
			t.Errorf("crash exit = %d", code)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("worker crash was not reported")
	}
	if mgr.Count() != 0 {
		t.Fatal("crashed worker retained")
	}
}

func TestIsolatedWorkerSpawnFailure(t *testing.T) {
	mgr := isolatedTestManager(t, "--pty-worker")
	_, err := mgr.Spawn(context.Background(), SpawnConfig{SessionID: "missing", Command: "openremote-nonexistent-command-abc"}, Hooks{})
	if err == nil {
		t.Fatal("missing command accepted")
	}
	if mgr.Count() != 0 {
		t.Fatal("failed spawn retained")
	}
}

func TestIsolatedWorkerClose(t *testing.T) {
	mgr := isolatedTestManager(t, "--pty-worker")
	command := "/bin/sh"
	if runtime.GOOS == "windows" {
		command = "cmd.exe"
	}
	inst, err := mgr.Spawn(context.Background(), SpawnConfig{SessionID: "close", Command: command}, Hooks{})
	if err != nil {
		t.Fatal(err)
	}
	mgr.Close()
	select {
	case <-inst.remote.done:
	case <-time.After(5 * time.Second):
		t.Fatal("worker survived manager shutdown")
	}
	if _, err := mgr.Spawn(context.Background(), SpawnConfig{SessionID: "late", Command: command}, Hooks{}); err == nil {
		t.Fatal("spawn after shutdown")
	}
}

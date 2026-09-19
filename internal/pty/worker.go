package pty

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"sync"
	"time"
)

type MsgType string

const (
	MsgSpawn   MsgType = "pty:spawn"
	MsgWrite   MsgType = "pty:write"
	MsgResize  MsgType = "pty:resize"
	MsgKill    MsgType = "pty:kill"
	MsgOutput  MsgType = "pty:output"
	MsgExit    MsgType = "pty:exit"
	MsgError   MsgType = "pty:error"
	MsgSpawned MsgType = "pty:spawned"
)

// Data is base64 in both directions, preserving arbitrary terminal bytes.
type IPCMessage struct {
	Type      MsgType           `json:"type"`
	SessionID string            `json:"sessionId,omitempty"`
	Command   string            `json:"command,omitempty"`
	Args      []string          `json:"args,omitempty"`
	CWD       string            `json:"cwd,omitempty"`
	Cols      int               `json:"cols,omitempty"`
	Rows      int               `json:"rows,omitempty"`
	Env       map[string]string `json:"env,omitempty"`
	Data      string            `json:"data,omitempty"`
	Code      *int              `json:"code,omitempty"`
	Signal    string            `json:"signal,omitempty"`
	Error     string            `json:"error,omitempty"`
}

// Supervisor runs one PTY in a separate worker, containing native terminal
// crashes to a single session. Failed jobs are never replayed automatically.
type Supervisor struct {
	bin      string
	args     []string
	mu       sync.Mutex
	input    io.WriteCloser
	encoder  *json.Encoder
	cmd      *exec.Cmd
	done     chan struct{}
	instance *Instance
	stopOnce sync.Once
}

func NewSupervisor(bin string, args ...string) *Supervisor {
	return &Supervisor{bin: bin, args: args, done: make(chan struct{})}
}

func (s *Supervisor) start(ctx context.Context, inst *Instance, hooks Hooks) error {
	s.instance = inst
	s.cmd = exec.CommandContext(ctx, s.bin, s.args...)
	s.cmd.Stderr = os.Stderr
	input, err := s.cmd.StdinPipe()
	if err != nil {
		return err
	}
	output, err := s.cmd.StdoutPipe()
	if err != nil {
		_ = input.Close()
		return err
	}
	s.input, s.encoder = input, json.NewEncoder(input)
	if err = s.cmd.Start(); err != nil {
		_ = input.Close()
		_ = output.Close()
		return err
	}
	inst.mu.Lock()
	inst.remote, inst.OnData, inst.OnExit = s, hooks.OnData, hooks.OnExit
	inst.mu.Unlock()
	ready := make(chan error, 1)
	go s.read(output, ready)
	cfg := inst.Config
	err = s.send(IPCMessage{Type: MsgSpawn, SessionID: cfg.SessionID, Command: cfg.Command, Args: cfg.Args, CWD: cfg.CWD, Cols: cfg.Cols, Rows: cfg.Rows, Env: cfg.Env})
	if err != nil {
		s.stop()
		return err
	}
	timer := time.NewTimer(15 * time.Second)
	defer timer.Stop()
	select {
	case err = <-ready:
		if err != nil {
			s.stop()
		}
		return err
	case <-ctx.Done():
		s.stop()
		return ctx.Err()
	case <-timer.C:
		s.stop()
		return fmt.Errorf("PTY worker startup timed out")
	}
}

func (s *Supervisor) read(output io.Reader, ready chan<- error) {
	defer close(s.done)
	dec := json.NewDecoder(output)
	notify := func(err error) {
		select {
		case ready <- err:
		default:
		}
	}
	exited := false
	for {
		var msg IPCMessage
		if err := dec.Decode(&msg); err != nil {
			break
		}
		switch msg.Type {
		case MsgSpawned:
			notify(nil)
		case MsgOutput:
			data, err := base64.StdEncoding.DecodeString(msg.Data)
			if err != nil {
				continue
			}
			s.instance.RingBuffer.Push(data)
			if s.instance.OnData != nil {
				s.instance.OnData(data)
			}
		case MsgError:
			notify(errors.New(msg.Error))
		case MsgExit:
			exited = true
			code := 0
			if msg.Code != nil {
				code = *msg.Code
			}
			s.instance.mu.Lock()
			s.instance.destroyed = true
			s.instance.mu.Unlock()
			s.instance.fireExit(code, msg.Signal)
			_ = s.input.Close()
		}
	}
	_ = s.input.Close()
	err := s.cmd.Wait()
	if err == nil {
		err = io.EOF
	}
	notify(fmt.Errorf("PTY worker stopped: %w", err))
	if !exited {
		s.instance.mu.Lock()
		s.instance.destroyed = true
		s.instance.mu.Unlock()
		s.instance.fireExit(-1, "worker-exit")
	}
}

func (s *Supervisor) send(msg IPCMessage) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	select {
	case <-s.done:
		return ErrNotFound
	default:
	}
	return s.encoder.Encode(msg)
}
func (s *Supervisor) write(data []byte) error {
	return s.send(IPCMessage{Type: MsgWrite, SessionID: s.instance.SessionID, Data: base64.StdEncoding.EncodeToString(data)})
}
func (s *Supervisor) stop() {
	s.stopOnce.Do(func() {
		// EOF asks the worker to close its manager without risking a blocked
		// write to a full IPC pipe before the shutdown deadline starts.
		_ = s.input.Close()
		select {
		case <-s.done:
		case <-time.After(3 * time.Second):
			_ = s.cmd.Process.Kill()
			<-s.done
		}
	})
}

// Worker owns native terminal handles; RunIO also supports transport tests.
type Worker struct{}

func NewWorker() *Worker                        { return &Worker{} }
func (w *Worker) Run(ctx context.Context) error { return w.RunIO(ctx, os.Stdin, os.Stdout) }
func (w *Worker) RunIO(ctx context.Context, input io.ReadCloser, output io.Writer) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	go func() { <-ctx.Done(); _ = input.Close() }()
	manager := NewManager()
	defer manager.Close()
	enc := json.NewEncoder(output)
	var outputMu sync.Mutex
	emit := func(msg IPCMessage) {
		outputMu.Lock()
		defer outputMu.Unlock()
		if err := enc.Encode(msg); err != nil {
			cancel()
		}
	}
	dec := json.NewDecoder(input)
	for {
		var msg IPCMessage
		if err := dec.Decode(&msg); err != nil {
			if errors.Is(err, io.EOF) || ctx.Err() != nil {
				return nil
			}
			return err
		}
		var err error
		switch msg.Type {
		case MsgSpawn:
			cfg := SpawnConfig{SessionID: msg.SessionID, Command: msg.Command, Args: msg.Args, CWD: msg.CWD, Cols: msg.Cols, Rows: msg.Rows, Env: msg.Env}
			_, err = manager.Spawn(ctx, cfg, Hooks{
				OnData: func(chunk []byte) {
					emit(IPCMessage{Type: MsgOutput, SessionID: cfg.SessionID, Data: base64.StdEncoding.EncodeToString(chunk)})
				},
				OnExit: func(code int, signal string) {
					emit(IPCMessage{Type: MsgExit, SessionID: cfg.SessionID, Code: &code, Signal: signal})
				},
			})
			if err == nil {
				emit(IPCMessage{Type: MsgSpawned, SessionID: cfg.SessionID})
			}
		case MsgWrite:
			var data []byte
			data, err = base64.StdEncoding.DecodeString(msg.Data)
			if err == nil {
				err = manager.Write(msg.SessionID, data)
			}
		case MsgResize:
			manager.Resize(msg.SessionID, msg.Cols, msg.Rows)
		case MsgKill:
			manager.Kill(msg.SessionID)
		default:
			err = fmt.Errorf("unknown worker message %q", msg.Type)
		}
		if err != nil {
			emit(IPCMessage{Type: MsgError, SessionID: msg.SessionID, Error: err.Error()})
		}
	}
}

package pty

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log"
	"os"
	"os/exec"
)

// Worker isolates PTY processes in a child `go run` process so a ConPTY/C++ panic
// never takes down the daemon. Protocol mirrors Node's IPC messages (spec 02 §2).

type MsgType string

const (
	MsgSpawn  MsgType = "pty:spawn"
	MsgWrite  MsgType = "pty:write"
	MsgResize MsgType = "pty:resize"
	MsgKill   MsgType = "pty:kill"
	MsgOutput MsgType = "pty:output"
	MsgExit   MsgType = "pty:exit"
	MsgError  MsgType = "pty:error"
	MsgSpawned MsgType = "pty:spawned"
)

type IPCMessage struct {
	Type      MsgType `json:"type"`
	SessionID string  `json:"sessionId,omitempty"`
	Command   string  `json:"command,omitempty"`
	Args      []string `json:"args,omitempty"`
	CWD       string  `json:"cwd,omitempty"`
	Cols      int     `json:"cols,omitempty"`
	Rows      int     `json:"rows,omitempty"`
	Env       map[string]string `json:"env,omitempty"`
	Data      string  `json:"data,omitempty"` // base64 for output, plain for write
	Code      *int    `json:"code,omitempty"`
	Signal    string  `json:"signal,omitempty"`
	Error     string  `json:"error,omitempty"`
}

// Supervisor — parent side: spawns/forwards to the worker subprocess
type Supervisor struct {
	cmd     *exec.Cmd
	enc     *json.Encoder
	dec     *json.Decoder
	OnOutput func(sessionID string, chunk []byte)
	OnExit   func(sessionID string, code int, signal string)
	OnError  func(err string)
}

func NewSupervisor(workerBin string, workerArgs ...string) *Supervisor {
	return &Supervisor{}
}

// Start launches the worker as a subprocess with JSON-lines over stdin/stdout.
// If workerBin == "" we run in-process (no isolation) — useful for tests.
type Worker struct {
	instances map[string]*Instance
}

func NewWorker() *Worker { return &Worker{instances: make(map[string]*Instance)} }

func (w *Worker) Run(ctx context.Context) error {
	dec := json.NewDecoder(os.Stdin)
	enc := json.NewEncoder(os.Stdout)
	for {
		var msg IPCMessage
		if err := dec.Decode(&msg); err != nil {
			if ctx.Err() != nil {
				return nil
			}
			// EOF — parent died
			return err
		}
		switch msg.Type {
		case MsgSpawn:
			cfg := SpawnConfig{
				SessionID: msg.SessionID, Command: msg.Command,
				Args: msg.Args, CWD: msg.CWD, Cols: msg.Cols, Rows: msg.Rows, Env: msg.Env,
			}
			hooks := Hooks{
				OnData: func(chunk []byte) {
					_ = enc.Encode(IPCMessage{Type: MsgOutput, SessionID: msg.SessionID, Data: base64.StdEncoding.EncodeToString(chunk)})
				},
				OnExit: func(code int, signal string) {
					c := code
					_ = enc.Encode(IPCMessage{Type: MsgExit, SessionID: msg.SessionID, Code: &c, Signal: signal})
					delete(w.instances, msg.SessionID)
				},
			}
			inst := NewInstance(cfg, 4*1024*1024)
			if err := inst.Spawn(ctx, hooks); err != nil {
				_ = enc.Encode(IPCMessage{Type: MsgError, SessionID: msg.SessionID, Error: err.Error()})
				continue
			}
			w.instances[msg.SessionID] = inst
			_ = enc.Encode(IPCMessage{Type: MsgSpawned, SessionID: msg.SessionID})

		case MsgWrite:
			if inst, ok := w.instances[msg.SessionID]; ok {
				_ = inst.Write([]byte(msg.Data))
			}
		case MsgResize:
			if inst, ok := w.instances[msg.SessionID]; ok {
				inst.Resize(msg.Cols, msg.Rows)
			}
		case MsgKill:
			if inst, ok := w.instances[msg.SessionID]; ok {
				inst.Kill()
				delete(w.instances, msg.SessionID)
			}
		default:
			log.Printf("[pty-worker] unknown msg type %q", msg.Type)
			_ = fmt.Errorf("unknown")
		}
	}
}

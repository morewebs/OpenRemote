package pty

import (
	"context"
	"io"
	"os"
	"sync"
	"time"

	ptylib "github.com/aymanbagabas/go-pty"
)

// ClampDimensions enforces sane terminal size — mirrors TS clampDimensions(20-300 cols, 5-100 rows)
func ClampDimensions(cols, rows int) (int, int) {
	if cols < 20 {
		cols = 20
	}
	if cols > 300 {
		cols = 300
	}
	if rows < 5 {
		rows = 5
	}
	if rows > 100 {
		rows = 100
	}
	return cols, rows
}

type SpawnConfig struct {
	SessionID string
	Command   string
	Args      []string
	CWD       string
	Cols      int
	Rows      int
	Env       map[string]string
}

// Hooks receive PTY output and process exit. Both must be installed before
// Spawn: the reader and reaper goroutines start inside Spawn, and a short-lived
// process can produce all its output and exit before Spawn's caller regains
// control.
type Hooks struct {
	OnData func(chunk []byte)
	OnExit func(code int, signal string)
}

// drainGrace bounds how long the exit notification waits for the reader to
// finish draining. Unix pty masters report EOF once the child exits, but ConPTY
// can hold the handle open, so exit must not block on the reader forever.
const drainGrace = 2 * time.Second

type Instance struct {
	SessionID  string
	Config     SpawnConfig
	RingBuffer *SlidingRingBuffer

	mu          sync.Mutex
	tty         ptylib.Pty
	cmd         *ptylib.Cmd
	destroyed   bool
	exitOnce    sync.Once
	consumeDone chan struct{}

	OnData func(chunk []byte)
	OnExit func(code int, signal string)
}

func NewInstance(cfg SpawnConfig, maxRingBytes int) *Instance {
	return &Instance{
		SessionID:   cfg.SessionID,
		Config:      cfg,
		RingBuffer:  NewSlidingRingBuffer(maxRingBytes),
		consumeDone: make(chan struct{}),
	}
}

func (p *Instance) Spawn(ctx context.Context, hooks Hooks) error {
	p.mu.Lock()
	if p.OnData != nil || p.OnExit != nil {
		p.mu.Unlock()
		return errHooksAlreadySet
	}
	p.OnData = hooks.OnData
	p.OnExit = hooks.OnExit
	p.mu.Unlock()

	cols, rows := ClampDimensions(p.Config.Cols, p.Config.Rows)

	tty, err := ptylib.New()
	if err != nil {
		return err
	}
	if err := tty.Resize(cols, rows); err != nil {
		tty.Close()
		return err
	}

	cmd := tty.CommandContext(ctx, p.Config.Command, p.Config.Args...)
	if p.Config.CWD != "" {
		cmd.Dir = p.Config.CWD
	}

	env := os.Environ()
	hasTerm := false
	for k := range p.Config.Env {
		if k == "TERM" {
			hasTerm = true
		}
	}
	for k, v := range p.Config.Env {
		env = append(env, k+"="+v)
	}
	// TERM must be set for ANSI-aware clients (ConPTY and unix alike)
	if !hasTerm {
		env = append(env, "TERM=xterm-256color")
	}
	cmd.Env = env

	if err := cmd.Start(); err != nil {
		tty.Close()
		return err
	}

	p.mu.Lock()
	p.tty = tty
	p.cmd = cmd
	p.mu.Unlock()

	// A real pty merges stdout and stderr into the master — one reader,
	// no interleaving race.
	go p.consume(tty)

	go func() {
		err := cmd.Wait()
		code := 0
		if err != nil {
			code = 1
			if state := cmd.ProcessState; state != nil {
				code = state.ExitCode()
			}
		}
		p.drainOutput()
		p.fireExit(code, "")
	}()

	return nil
}

func (p *Instance) consume(r io.Reader) {
	defer close(p.consumeDone)
	buf := make([]byte, 8192)
	for {
		n, err := r.Read(buf)
		if n > 0 {
			chunk := make([]byte, n)
			copy(chunk, buf[:n])
			p.RingBuffer.Push(chunk)
			p.mu.Lock()
			onData := p.OnData
			p.mu.Unlock()
			if onData != nil {
				onData(chunk)
			}
		}
		if err != nil {
			break
		}
	}
}

// drainOutput waits for the consume goroutine to hand off all pending output,
// bounded by drainGrace so a pty handle that stays readable after the child is
// gone (possible under ConPTY) cannot block exit notification indefinitely.
func (p *Instance) drainOutput() {
	select {
	case <-p.consumeDone:
	case <-time.After(drainGrace):
	}
}

// fireExit delivers OnExit exactly once across the Wait goroutine and Kill.
func (p *Instance) fireExit(code int, signal string) {
	p.exitOnce.Do(func() {
		p.mu.Lock()
		onExit := p.OnExit
		p.mu.Unlock()
		if onExit != nil {
			onExit(code, signal)
		}
	})
}

var errHooksAlreadySet = errNotFound("pty hooks already installed")

func (p *Instance) Write(data []byte) error {
	p.mu.Lock()
	if p.destroyed || p.tty == nil {
		p.mu.Unlock()
		return nil
	}
	w := p.tty
	p.mu.Unlock()
	_, err := w.Write(data)
	return err
}

func (p *Instance) Resize(cols, rows int) {
	cols, rows = ClampDimensions(cols, rows)
	p.mu.Lock()
	tty := p.tty
	destroyed := p.destroyed
	p.mu.Unlock()
	if tty == nil || destroyed {
		return
	}
	_ = tty.Resize(cols, rows)
}

func (p *Instance) Kill() {
	p.mu.Lock()
	if p.destroyed {
		p.mu.Unlock()
		return
	}
	p.destroyed = true
	tty := p.tty
	cmd := p.cmd
	p.mu.Unlock()

	// Close the pty master before killing the process so the reader
	// goroutine unblocks on Windows (a closed ConPTY handle fails reads).
	if tty != nil {
		_ = tty.Close()
	}
	if cmd != nil && cmd.Process != nil {
		_ = cmd.Process.Kill()
	}
	if cmd != nil {
		p.drainOutput()
	}
	p.fireExit(0, "SIGKILL")
}

package pty

import (
	"context"
	"io"
	"os"
	"os/exec"
	"sync"
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

type Instance struct {
	SessionID  string
	Config     SpawnConfig
	RingBuffer *SlidingRingBuffer

	mu        sync.Mutex
	cmd       *exec.Cmd
	stdin     io.WriteCloser
	cancel    context.CancelFunc
	destroyed bool

	OnData func(chunk []byte)
	OnExit func(code int, signal string)
}

func NewInstance(cfg SpawnConfig, maxRingBytes int) *Instance {
	return &Instance{
		SessionID:  cfg.SessionID,
		Config:     cfg,
		RingBuffer: NewSlidingRingBuffer(maxRingBytes),
	}
}

func (p *Instance) Spawn(ctx context.Context) error {
	cols, rows := ClampDimensions(p.Config.Cols, p.Config.Rows)
	_ = cols
	_ = rows // used when we swap to conpty; today plain pipes

	cctx, cancel := context.WithCancel(ctx)
	p.cancel = cancel

	cmd := exec.CommandContext(cctx, p.Config.Command, p.Config.Args...)
	if p.Config.CWD != "" {
		cmd.Dir = p.Config.CWD
	}
	env := os.Environ()
	for k, v := range p.Config.Env {
		env = append(env, k+"="+v)
	}
	// ensure TERM for ANSI apps
	hasTerm := false
	for k := range p.Config.Env {
		if k == "TERM" {
			hasTerm = true
			break
		}
	}
	if !hasTerm {
		env = append(env, "TERM=xterm-256color")
	}
	cmd.Env = env

	stdin, err := cmd.StdinPipe()
	if err != nil {
		cancel()
		return err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		cancel()
		return err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		cancel()
		return err
	}
	p.stdin = stdin
	p.cmd = cmd

	if err := cmd.Start(); err != nil {
		cancel()
		return err
	}

	consume := func(r io.Reader) {
		buf := make([]byte, 8192)
		for {
			n, err := r.Read(buf)
			if n > 0 {
				chunk := make([]byte, n)
				copy(chunk, buf[:n])
				p.RingBuffer.Push(chunk)
				if p.OnData != nil {
					p.OnData(chunk)
				}
			}
			if err != nil {
				break
			}
		}
	}
	go consume(stdout)
	go consume(stderr)

	go func() {
		err := cmd.Wait()
		code := 0
		if err != nil {
			if exitErr, ok := err.(*exec.ExitError); ok {
				code = exitErr.ExitCode()
			} else {
				code = 1
			}
		}
		if p.OnExit != nil && !p.isDestroyed() {
			p.OnExit(code, "")
		}
	}()

	return nil
}

func (p *Instance) isDestroyed() bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.destroyed
}

func (p *Instance) Write(data []byte) error {
	p.mu.Lock()
	if p.destroyed || p.stdin == nil {
		p.mu.Unlock()
		return nil
	}
	w := p.stdin
	p.mu.Unlock()
	_, err := w.Write(data)
	return err
}

func (p *Instance) Resize(cols, rows int) {
	// plain pipes: no PTY resize; kept for API parity.
	// When conpty/creack/pty is added, call pty.Setsize here.
}

func (p *Instance) Kill() {
	p.mu.Lock()
	if p.destroyed {
		p.mu.Unlock()
		return
	}
	p.destroyed = true
	cancel := p.cancel
	cmd := p.cmd
	stdin := p.stdin
	p.mu.Unlock()

	if stdin != nil {
		_ = stdin.Close()
	}
	if cancel != nil {
		cancel()
	}
	if cmd != nil && cmd.Process != nil {
		_ = cmd.Process.Kill()
	}
	if p.OnExit != nil {
		p.OnExit(0, "SIGKILL")
	}
}

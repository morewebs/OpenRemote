package ptybase

import (
	"context"
	"fmt"
	"sync"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/core/screen"
	"github.com/morewebs/OpenRemote/internal/driver/types"
	"github.com/morewebs/OpenRemote/internal/pty"
)

// BaseSession provides the standard PTY, Screen emulator, and Chat Assembler pipeline.
type BaseSession struct {
	mu              sync.Mutex
	cfg             types.SessionConfig
	instance        *pty.Instance
	screen          *screen.Screen
	assembler       *chat.Assembler
	sink            types.Sink
	promptFormatter func(string) []byte
	approveKey      func(bool) []byte
}

type Opts struct {
	Command         string
	Args            []string
	Lexer           chat.Lexer
	PromptFormatter func(string) []byte
	ApproveKey      func(approved bool) []byte
	// LineHook is invoked on every committed screen line and may return
	// driver-specific structured events (approvals, auth URLs, diffs, ...).
	LineHook func(sessionID, line string) []any
}

// Start launches a new PTY session wired into Screen and Assembler.
func Start(ctx context.Context, cfg types.SessionConfig, mgr *pty.Manager, sink types.Sink, opts Opts) (*BaseSession, error) {
	cols := cfg.Cols
	if cols <= 0 {
		cols = 120
	}
	rows := cfg.Rows
	if rows <= 0 {
		rows = 30
	}

	targetCWD := cfg.CWD
	if cfg.WorktreePath != "" {
		targetCWD = cfg.WorktreePath
	}

	scr := screen.New(cols, rows)

	var asm *chat.Assembler
	if opts.Lexer != nil {
		asm = chat.NewAssembler(cfg.SessionID, opts.Lexer, func(msg chat.Message) {
			if sink != nil {
				sink.Message(msg)
			}
		})
	}
	scr.OnCommit(func(line string) {
		if asm != nil {
			asm.Feed(line)
		}
		if opts.LineHook != nil {
			for _, ev := range opts.LineHook(cfg.SessionID, line) {
				if sink != nil {
					sink.Event(ev)
				}
			}
		}
	})

	promptFmt := opts.PromptFormatter
	if promptFmt == nil {
		promptFmt = func(p string) []byte {
			return []byte(p + "\r\n")
		}
	}

	approveFmt := opts.ApproveKey
	if approveFmt == nil {
		approveFmt = func(approved bool) []byte {
			if approved {
				return []byte("y\r\n")
			}
			return []byte("n\r\n")
		}
	}

	bs := &BaseSession{
		cfg:             cfg,
		screen:          scr,
		assembler:       asm,
		sink:            sink,
		promptFormatter: promptFmt,
		approveKey:      approveFmt,
	}

	spawnCfg := pty.SpawnConfig{
		SessionID: cfg.SessionID,
		Command:   opts.Command,
		Args:      opts.Args,
		CWD:       targetCWD,
		Cols:      cols,
		Rows:      rows,
		Env:       cfg.Env,
	}

	inst, err := mgr.Spawn(ctx, spawnCfg)
	if err != nil {
		return nil, fmt.Errorf("pty spawn failed: %w", err)
	}

	inst.OnData = func(chunk []byte) {
		if sink != nil {
			sink.Bytes(chunk)
		}
		scr.Write(chunk)
	}

	inst.OnExit = func(code int, signal string) {
		if asm != nil {
			asm.Flush()
		}
		scr.FlushCurrentScreenLines()
		if sink != nil {
			sink.Exit(code, signal)
		}
	}

	bs.instance = inst
	return bs, nil
}

func (s *BaseSession) Prompt(text string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.instance == nil {
		return fmt.Errorf("session not running")
	}
	payload := s.promptFormatter(text)
	return s.instance.Write(payload)
}

func (s *BaseSession) RawInput(data []byte) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.instance == nil {
		return fmt.Errorf("session not running")
	}
	return s.instance.Write(data)
}

func (s *BaseSession) Resize(cols, rows int) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.instance != nil {
		s.instance.Resize(cols, rows)
	}
	if s.screen != nil {
		s.screen.Resize(cols, rows)
	}
	return nil
}

func (s *BaseSession) Snapshot() []byte {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.instance != nil && s.instance.RingBuffer != nil {
		return s.instance.RingBuffer.ReadAll()
	}
	return nil
}

func (s *BaseSession) Approve(_ string, approved bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.instance == nil {
		return fmt.Errorf("session not running")
	}
	return s.instance.Write(s.approveKey(approved))
}

func (s *BaseSession) Answer(_ string, answer any) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.instance == nil {
		return fmt.Errorf("session not running")
	}
	ansStr := fmt.Sprintf("%v\r\n", answer)
	return s.instance.Write([]byte(ansStr))
}

func (s *BaseSession) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.instance != nil {
		s.instance.Kill()
	}
	return nil
}

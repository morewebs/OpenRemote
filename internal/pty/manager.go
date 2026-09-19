package pty

import (
	"context"
	"github.com/morewebs/OpenRemote/internal/core/supervisor"
	"sync"
	"time"
)

// Manager owns live PTYs, optionally isolated in one worker per session.
type Manager struct {
	mu         sync.RWMutex
	instances  map[string]*Instance
	workerBin  string
	workerArgs []string
	closed     bool
	spawning   sync.WaitGroup
	breaker    *supervisor.CircuitBreaker
}

func NewManager() *Manager {
	return &Manager{instances: make(map[string]*Instance), breaker: supervisor.NewCircuitBreaker(3, 15*time.Minute)}
}

func NewIsolatedManager(workerBin string, workerArgs ...string) *Manager {
	m := NewManager()
	m.workerBin, m.workerArgs = workerBin, workerArgs
	return m
}

// Spawn creates an instance with its hooks already wired, so output produced by
// a short-lived process cannot be lost before the caller installs them.
func (m *Manager) Spawn(ctx context.Context, cfg SpawnConfig, hooks Hooks) (*Instance, error) {
	if !m.breaker.Allow(cfg.Command) {
		return nil, errNotFound("PTY worker circuit breaker open after 3 crashes in 15 minutes")
	}
	inst := NewInstance(cfg, 4*1024*1024)
	m.mu.Lock()
	if m.closed {
		m.mu.Unlock()
		return nil, ErrNotFound
	}
	if _, exists := m.instances[cfg.SessionID]; exists {
		m.mu.Unlock()
		return nil, errNotFound("session already exists")
	}
	m.instances[cfg.SessionID] = inst
	m.spawning.Add(1)
	m.mu.Unlock()
	defer m.spawning.Done()
	onExit := hooks.OnExit
	hooks.OnExit = func(code int, signal string) {
		if signal == "worker-exit" {
			m.breaker.Failure(cfg.Command)
		}
		m.mu.Lock()
		if m.instances[cfg.SessionID] == inst {
			delete(m.instances, cfg.SessionID)
		}
		m.mu.Unlock()
		if onExit != nil {
			onExit(code, signal)
		}
	}
	var err error
	if m.workerBin == "" {
		err = inst.Spawn(ctx, hooks)
	} else {
		err = NewSupervisor(m.workerBin, m.workerArgs...).start(ctx, inst, hooks)
	}
	if err != nil {
		m.mu.Lock()
		if m.instances[cfg.SessionID] == inst {
			delete(m.instances, cfg.SessionID)
		}
		m.mu.Unlock()
		return nil, err
	}
	m.mu.RLock()
	closed := m.closed
	m.mu.RUnlock()
	if closed {
		inst.Kill()
		return nil, ErrNotFound
	}
	return inst, nil
}

func (m *Manager) Close() {
	m.mu.Lock()
	m.closed = true
	m.mu.Unlock()
	m.spawning.Wait()
	m.mu.Lock()
	instances := m.instances
	m.instances = make(map[string]*Instance)
	m.mu.Unlock()
	for _, inst := range instances {
		inst.Kill()
	}
}

func (m *Manager) Get(sessionID string) (*Instance, bool) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	inst, ok := m.instances[sessionID]
	return inst, ok
}

func (m *Manager) Write(sessionID string, data []byte) error {
	inst, ok := m.Get(sessionID)
	if !ok {
		return ErrNotFound
	}
	return inst.Write(data)
}

func (m *Manager) Resize(sessionID string, cols, rows int) {
	if inst, ok := m.Get(sessionID); ok {
		inst.Resize(cols, rows)
	}
}

func (m *Manager) Kill(sessionID string) {
	m.mu.Lock()
	inst, ok := m.instances[sessionID]
	if ok {
		delete(m.instances, sessionID)
	}
	m.mu.Unlock()
	if ok {
		inst.Kill()
	}
}

func (m *Manager) Count() int {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return len(m.instances)
}

var ErrNotFound = errNotFound("session not found")

type errNotFound string

func (e errNotFound) Error() string { return string(e) }

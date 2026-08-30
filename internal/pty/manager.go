package pty

import (
	"context"
	"sync"
)

// Manager multiplexes PTY instances — in-process supervisor.
// When isolated worker mode is added, this becomes the IPC client to the worker subprocess.
type Manager struct {
	mu        sync.RWMutex
	instances map[string]*Instance
}

func NewManager() *Manager { return &Manager{instances: make(map[string]*Instance)} }

// Spawn creates an instance with its hooks already wired, so output produced by
// a short-lived process cannot be lost before the caller installs them.
func (m *Manager) Spawn(ctx context.Context, cfg SpawnConfig, hooks Hooks) (*Instance, error) {
	inst := NewInstance(cfg, 4*1024*1024)
	if err := inst.Spawn(ctx, hooks); err != nil {
		return nil, err
	}
	m.mu.Lock()
	m.instances[cfg.SessionID] = inst
	m.mu.Unlock()
	return inst, nil
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

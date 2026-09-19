package approval

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"fmt"
	"strings"
	"sync"
	"time"
)

// PendingApproval tracks an in-flight approval requested by an agent.
type PendingApproval struct {
	ID                string    `json:"id"`
	SessionID         string    `json:"sessionId"`
	ToolName          string    `json:"toolName"`
	Command           string    `json:"command"`
	Description       string    `json:"description,omitempty"`
	AutoDenyTimeoutMs int       `json:"autoDenyTimeoutMs"`
	CreatedAt         time.Time `json:"createdAt"`
	ExpiresAt         time.Time `json:"expiresAt"`
	Resolved          bool      `json:"resolved"`
	Approved          bool      `json:"approved"`
	ResolvedBy        string    `json:"resolvedBy,omitempty"`
	done              chan struct{}
	resolvedAt        time.Time
	resolving         bool
}

// GenerateID produces a deterministic approval ID based on session and command/prompt text.
func GenerateID(sessionID, prompt string) string {
	normalized := strings.TrimSpace(prompt)
	h := sha1.New()
	h.Write([]byte(sessionID + "|" + normalized))
	return "apr_" + hex.EncodeToString(h.Sum(nil))[:10]
}

// Registry stores and coordinates approvals for all sessions.
type Registry struct {
	mu        sync.RWMutex
	approvals map[string]*PendingApproval
	onExpire  func(app *PendingApproval)
	done      chan struct{}
	closeOnce sync.Once
}

func NewRegistry(onExpire func(app *PendingApproval)) *Registry {
	r := &Registry{
		approvals: make(map[string]*PendingApproval),
		onExpire:  onExpire,
		done:      make(chan struct{}),
	}
	go r.reaperLoop()
	return r
}

func (r *Registry) Put(app *PendingApproval) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if previous, ok := r.approvals[app.ID]; ok && !previous.Resolved {
		return
	}
	copy := *app
	app = &copy

	if app.CreatedAt.IsZero() {
		app.CreatedAt = time.Now()
	}
	if app.AutoDenyTimeoutMs <= 0 {
		app.AutoDenyTimeoutMs = 120000 // 2 min default
	}
	app.ExpiresAt = app.CreatedAt.Add(time.Duration(app.AutoDenyTimeoutMs) * time.Millisecond)
	app.done = make(chan struct{})

	r.approvals[app.ID] = app
}

func (r *Registry) Get(id string) (*PendingApproval, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	app, ok := r.approvals[id]
	if !ok {
		return nil, false
	}
	copy := *app
	return &copy, true
}

func (r *Registry) Resolve(id string, approved bool, resolvedBy string) (*PendingApproval, error) {
	return r.ResolveWith(id, approved, resolvedBy, nil)
}

// ResolveWith commits the resolution only when delivery to the agent succeeds.
func (r *Registry) ResolveWith(id string, approved bool, resolvedBy string, deliver func(*PendingApproval) error) (*PendingApproval, error) {
	r.mu.Lock()

	app, ok := r.approvals[id]
	if !ok {
		r.mu.Unlock()
		return nil, fmt.Errorf("approval %q not found", id)
	}
	if app.Resolved || app.resolving {
		r.mu.Unlock()
		return nil, fmt.Errorf("approval %q already resolved or being delivered", id)
	}
	app.resolving = true
	copy := *app
	r.mu.Unlock()
	var err error
	if deliver != nil {
		err = deliver(&copy)
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	app.resolving = false
	if err != nil {
		return nil, err
	}

	app.Resolved = true
	app.Approved = approved
	app.ResolvedBy = resolvedBy
	app.resolvedAt = time.Now()
	close(app.done)
	copy = *app
	return &copy, nil
}

func (r *Registry) Wait(ctx context.Context, id string) (bool, error) {
	r.mu.RLock()
	app, ok := r.approvals[id]
	r.mu.RUnlock()
	if !ok {
		return false, fmt.Errorf("approval %q not found", id)
	}
	select {
	case <-ctx.Done():
		return false, ctx.Err()
	case <-r.done:
		return false, fmt.Errorf("approval registry closed")
	case <-app.done:
		r.mu.RLock()
		defer r.mu.RUnlock()
		return app.Approved, nil
	}
}

func (r *Registry) Close() { r.closeOnce.Do(func() { close(r.done) }) }

func (r *Registry) List(sessionID string) []*PendingApproval {
	r.mu.RLock()
	defer r.mu.RUnlock()

	var res []*PendingApproval
	for _, app := range r.approvals {
		if sessionID == "" || app.SessionID == sessionID {
			copy := *app
			res = append(res, &copy)
		}
	}
	return res
}

func (r *Registry) reaperLoop() {
	ticker := time.NewTicker(1 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-r.done:
			return
		case <-ticker.C:
		}
		now := time.Now()
		var expired []*PendingApproval

		r.mu.Lock()
		for id, app := range r.approvals {
			if app.Resolved && now.Sub(app.resolvedAt) > time.Minute {
				delete(r.approvals, id)
				continue
			}
			if !app.Resolved && !app.resolving && now.After(app.ExpiresAt) {
				app.Resolved = true
				app.Approved = false
				app.ResolvedBy = "timeout"
				app.resolvedAt = now
				close(app.done)
				copy := *app
				expired = append(expired, &copy)
			}
		}
		r.mu.Unlock()

		if r.onExpire != nil {
			for _, app := range expired {
				r.onExpire(app)
			}
		}
	}
}

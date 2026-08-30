package approval

import (
	"crypto/sha1"
	"encoding/hex"
	"fmt"
	"strings"
	"sync"
	"time"
)

// PendingApproval tracks an in-flight approval requested by an agent.
type PendingApproval struct {
	ID                string        `json:"id"`
	SessionID         string        `json:"sessionId"`
	ToolName          string        `json:"toolName"`
	Command           string        `json:"command"`
	Description       string        `json:"description,omitempty"`
	AutoDenyTimeoutMs int           `json:"autoDenyTimeoutMs"`
	CreatedAt         time.Time     `json:"createdAt"`
	ExpiresAt         time.Time     `json:"expiresAt"`
	Resolved          bool          `json:"resolved"`
	Approved          bool          `json:"approved"`
	ResolvedBy        string        `json:"resolvedBy,omitempty"`
	replyCh           chan bool
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
}

func NewRegistry(onExpire func(app *PendingApproval)) *Registry {
	r := &Registry{
		approvals: make(map[string]*PendingApproval),
		onExpire:  onExpire,
	}
	go r.reaperLoop()
	return r
}

func (r *Registry) Put(app *PendingApproval) {
	r.mu.Lock()
	defer r.mu.Unlock()

	if app.CreatedAt.IsZero() {
		app.CreatedAt = time.Now()
	}
	if app.AutoDenyTimeoutMs <= 0 {
		app.AutoDenyTimeoutMs = 120000 // 2 min default
	}
	app.ExpiresAt = app.CreatedAt.Add(time.Duration(app.AutoDenyTimeoutMs) * time.Millisecond)
	app.replyCh = make(chan bool, 1)

	r.approvals[app.ID] = app
}

func (r *Registry) Get(id string) (*PendingApproval, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	app, ok := r.approvals[id]
	return app, ok
}

func (r *Registry) Resolve(id string, approved bool, resolvedBy string) (*PendingApproval, error) {
	r.mu.Lock()
	defer r.mu.Unlock()

	app, ok := r.approvals[id]
	if !ok {
		return nil, fmt.Errorf("approval %q not found", id)
	}
	if app.Resolved {
		return app, fmt.Errorf("approval %q already resolved", id)
	}

	app.Resolved = true
	app.Approved = approved
	app.ResolvedBy = resolvedBy

	select {
	case app.replyCh <- approved:
	default:
	}

	return app, nil
}

func (r *Registry) List(sessionID string) []*PendingApproval {
	r.mu.RLock()
	defer r.mu.RUnlock()

	var res []*PendingApproval
	for _, app := range r.approvals {
		if sessionID == "" || app.SessionID == sessionID {
			res = append(res, app)
		}
	}
	return res
}

func (r *Registry) reaperLoop() {
	ticker := time.NewTicker(1 * time.Second)
	defer ticker.Stop()

	for range ticker.C {
		now := time.Now()
		var expired []*PendingApproval

		r.mu.Lock()
		for _, app := range r.approvals {
			if !app.Resolved && now.After(app.ExpiresAt) {
				app.Resolved = true
				app.Approved = false
				app.ResolvedBy = "timeout"
				select {
				case app.replyCh <- false:
				default:
				}
				expired = append(expired, app)
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

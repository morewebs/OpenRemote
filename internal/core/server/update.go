package server

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/update"
)

// serverUpdater layers apply-progress state over the release-check manager.
type serverUpdater struct {
	*update.Manager
	mu          sync.Mutex
	applying    bool
	applyError  string
	restartHook func() // invoked after a successful apply+shutdown; exits the worker in production
}

// handleUpdate serves the update status and apply operations:
//
//	GET  /api/v1/update  -> cached release-check status + apply state
//	POST /api/v1/update  -> 202, apply in background: download, verify,
//	                        swap, graceful shutdown, then restart via the
//	                        supervisor handshake
func (s *Server) handleUpdate(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(s.updateStatus())
	case http.MethodPost:
		if !s.updaterStart() {
			http.Error(w, `{"code":"ERR_UPDATE_IN_PROGRESS"}`, http.StatusConflict)
			return
		}
		w.WriteHeader(http.StatusAccepted)
		go s.updateApply()
	default:
		methodNotAllowed(w, http.MethodGet, http.MethodPost)
	}
}

func (s *Server) updateStatus() map[string]any {
	st := s.updates.Status()
	s.updates.mu.Lock()
	applying, applyErr := s.updates.applying, s.updates.applyError
	s.updates.mu.Unlock()
	auto := "off"
	if s.cfg.UpdateCheckInterval > 0 {
		auto = s.cfg.UpdateCheckInterval.String()
	}
	return map[string]any{
		"current":    st.Current,
		"latest":     st.Latest,
		"available":  st.Available,
		"checkedAt":  st.CheckedAt.Format(time.RFC3339),
		"autoCheck":  auto,
		"applying":   applying,
		"applyError": applyErr,
	}
}

// updaterStart claims the applying slot so a second POST cannot double-apply.
func (s *Server) updaterStart() bool {
	s.updates.mu.Lock()
	defer s.updates.mu.Unlock()
	if s.updates.applying {
		return false
	}
	s.updates.applying, s.updates.applyError = true, ""
	return true
}

func (s *Server) updateApply() {
	defer func() {
		s.updates.mu.Lock()
		s.updates.applying = false
		s.updates.mu.Unlock()
	}()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Minute)
	defer cancel()
	if err := s.updates.Apply(ctx); err != nil {
		if errors.Is(err, update.ErrRestartNeeded) {
			log.Printf("[update] applied %s; restarting daemon", s.updates.Status().Latest)
			shutdownCtx, shutdownCancel := context.WithTimeout(context.Background(), 15*time.Second)
			_ = s.Shutdown(shutdownCtx)
			shutdownCancel()
			if s.updates.restartHook != nil {
				s.updates.restartHook() // os.Exit(RestartExitCode) in production
			}
			return
		}
		s.updates.mu.Lock()
		s.updates.applyError = err.Error()
		s.updates.mu.Unlock()
		log.Printf("[update] apply failed: %v", err)
	}
}

// startUpdateChecker runs the periodic release check (interval 0 = disabled,
// as are unstamped dev builds). The first check happens at startup so the
// status endpoint reflects reality without waiting a full interval.
func (s *Server) startUpdateChecker() {
	if s.cfg.UpdateCheckInterval <= 0 || s.cfg.Version == "" || s.cfg.Version == "dev" {
		return
	}
	go func() {
		check := func() {
			ctx, cancel := context.WithTimeout(s.ctx, 60*time.Second)
			defer cancel()
			if err := s.updates.Check(ctx); err != nil {
				log.Printf("[update] check failed: %v", err)
				return
			}
			if st := s.updates.Status(); st.Available {
				log.Printf("[update] update available: %s (current %s); apply via the UI, `openremote update`, or POST /api/v1/update", st.Latest, st.Current)
			}
		}
		check()
		ticker := time.NewTicker(s.cfg.UpdateCheckInterval)
		defer ticker.Stop()
		for {
			select {
			case <-s.ctx.Done():
				return
			case <-ticker.C:
				check()
			}
		}
	}()
}

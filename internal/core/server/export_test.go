package server

import (
	"time"

	"github.com/morewebs/OpenRemote/internal/core/parser"
	"github.com/morewebs/OpenRemote/internal/driver"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

// TestInjectSession registers an in-memory session backed by the given driver
// session and upserts its database row so events can be appended (FK-safe).
func (s *Server) TestInjectSession(id string, sess driver.Session) *SessionState {
	if err := s.bus.UpsertSession(id, "ws-test", string(protocol.AgentShell), s.cfg.DataDir, "", "", "", string(protocol.StatusRunning)); err != nil {
		panic(err)
	}
	st := &SessionState{
		SessionID:   id,
		WorkspaceID: "ws-test",
		AgentID:     protocol.AgentShell,
		CWD:         s.cfg.DataDir,
		Status:      protocol.StatusRunning,
		CreatedAt:   time.Now().UnixMilli(),
		DriverSess:  sess,
		Parser:      parser.NewStreamParser(id),
		Hub:         NewHub(),
	}
	s.mu.Lock()
	s.sessions[id] = st
	s.mu.Unlock()
	return st
}

// TestSink returns the server's sink for an injected session so tests can
// drive the same Bytes/Event pipeline a real driver uses.
func (s *Server) TestSink(sessionID string) driver.Sink {
	s.mu.RLock()
	st := s.sessions[sessionID]
	s.mu.RUnlock()
	return &serverSink{server: s, sessionID: sessionID, hub: st.Hub, parser: st.Parser}
}

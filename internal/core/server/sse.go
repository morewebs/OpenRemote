package server

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"
	"time"
)

func (s *Server) handleSSE(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		methodNotAllowed(w, http.MethodGet)
		return
	}
	id := r.URL.Query().Get("sessionId")
	s.mu.RLock()
	state := s.sessions[id]
	s.mu.RUnlock()
	if state == nil {
		http.Error(w, "session not found", http.StatusNotFound)
		return
	}
	cursor := r.URL.Query().Get("lastSeq")
	if header := r.Header.Get("Last-Event-ID"); header != "" {
		cursor = header
	}
	var lastSeq int64
	if cursor != "" {
		var err error
		lastSeq, err = strconv.ParseInt(cursor, 10, 64)
		if err != nil || lastSeq < 0 {
			http.Error(w, "invalid event cursor", http.StatusBadRequest)
			return
		}
	}
	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", http.StatusInternalServerError)
		return
	}
	// Subscribe before the durable replay. Notifications are only wakeups;
	// SQLite is the source of truth, removing the replay/live handoff gap.
	client := &wsClient{send: make(chan []byte, 128), done: make(chan struct{})}
	state.Hub.Add(client)
	defer state.Hub.Remove(client)
	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("X-Accel-Buffering", "no")
	drain := func() bool {
		events, err := s.bus.GetEventsSince(id, lastSeq)
		if err != nil {
			return false
		}
		for _, event := range events {
			seq, _ := event["seq"].(int64)
			data, err := json.Marshal(event)
			if err != nil {
				return false
			}
			if _, err := fmt.Fprintf(w, "id: %d\ndata: %s\n\n", seq, data); err != nil {
				return false
			}
			lastSeq = seq
		}
		flusher.Flush()
		return true
	}
	if !drain() {
		return
	}
	ticker := time.NewTicker(20 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-r.Context().Done():
			return
		case <-client.done:
			return
		case <-client.send:
			if !drain() {
				return
			}
		case <-ticker.C:
			if _, err := fmt.Fprint(w, ": keepalive\n\n"); err != nil {
				return
			}
			flusher.Flush()
		}
	}
}

package events

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"

	_ "modernc.org/sqlite"
)

// Bus is the SQLite WAL monotonic event store — spec 02 §3.
// Every session has an independent monotonically increasing seq (AUTOINCREMENT).
type Bus struct {
	mu sync.Mutex
	db *sql.DB
}

func Open(dataDir string) (*Bus, error) {
	if err := os.MkdirAll(dataDir, 0o700); err != nil {
		return nil, err
	}
	path := filepath.Join(dataDir, "events.db")
	db, err := sql.Open("sqlite", path+"?cache=shared")
	if err != nil {
		return nil, err
	}
	for _, pragma := range []string{
		`PRAGMA journal_mode=WAL;`,
		`PRAGMA synchronous=NORMAL;`,
		`PRAGMA foreign_keys=ON;`,
	} {
		if _, err := db.Exec(pragma); err != nil {
			return nil, fmt.Errorf("pragma %q: %w", pragma, err)
		}
	}
	if err := migrate(db); err != nil {
		return nil, err
	}
	return &Bus{db: db}, nil
}

func migrate(db *sql.DB) error {
	_, err := db.Exec(`
	CREATE TABLE IF NOT EXISTS sessions (
		session_id   TEXT PRIMARY KEY,
		workspace_id TEXT NOT NULL,
		agent_id     TEXT NOT NULL,
		cwd          TEXT NOT NULL,
		worktree_path TEXT,
		branch_name  TEXT,
		created_at   INTEGER NOT NULL,
		updated_at   INTEGER NOT NULL,
		status       TEXT NOT NULL
	);
	CREATE TABLE IF NOT EXISTS events (
		seq        INTEGER PRIMARY KEY AUTOINCREMENT,
		session_id TEXT NOT NULL,
		event_type TEXT NOT NULL,
		payload    TEXT NOT NULL,
		created_at INTEGER NOT NULL,
		FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
	);
	CREATE INDEX IF NOT EXISTS idx_events_session_seq ON events(session_id, seq);
	`)
	return err
}

func (b *Bus) Close() error { return b.db.Close() }

func (b *Bus) UpsertSession(sessionID, workspaceID, agentID, cwd, worktreePath, branch, status string) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	now := time.Now().UnixMilli()
	_, err := b.db.Exec(`
		INSERT INTO sessions(session_id,workspace_id,agent_id,cwd,worktree_path,branch_name,created_at,updated_at,status)
		VALUES(?,?,?,?,?,?,?,?,?)
		ON CONFLICT(session_id) DO UPDATE SET
			workspace_id=excluded.workspace_id, agent_id=excluded.agent_id,
			cwd=excluded.cwd, worktree_path=excluded.worktree_path,
			branch_name=excluded.branch_name, updated_at=excluded.updated_at,
			status=excluded.status
	`, sessionID, workspaceID, agentID, cwd, worktreePath, branch, now, now, status)
	return err
}

func (b *Bus) UpdateSessionStatus(sessionID, status string) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	_, err := b.db.Exec(`UPDATE sessions SET status=?, updated_at=? WHERE session_id=?`, status, time.Now().UnixMilli(), sessionID)
	return err
}

func (b *Bus) DeleteSession(sessionID string) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	_, err := b.db.Exec(`DELETE FROM sessions WHERE session_id=?`, sessionID)
	return err
}

func (b *Bus) ListSessions() ([]map[string]any, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	rows, err := b.db.Query(`SELECT session_id,workspace_id,agent_id,cwd,worktree_path,branch_name,created_at,updated_at,status FROM sessions ORDER BY created_at DESC`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []map[string]any
	for rows.Next() {
		var sID, wID, aID, cwd, wt, br, status string
		var sqlWT, sqlBr sql.NullString
		var created, updated int64
		if err := rows.Scan(&sID, &wID, &aID, &cwd, &sqlWT, &sqlBr, &created, &updated, &status); err != nil {
			return nil, err
		}
		if sqlWT.Valid {
			wt = sqlWT.String
		}
		if sqlBr.Valid {
			br = sqlBr.String
		}
		out = append(out, map[string]any{
			"sessionId": sID, "workspaceId": wID, "agentId": aID, "cwd": cwd,
			"worktreePath": wt, "branchName": br, "createdAt": created, "updatedAt": updated, "status": status,
		})
	}
	return out, rows.Err()
}

func (b *Bus) AppendEvent(sessionID, eventType string, payload any) (int64, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	raw, err := json.Marshal(payload)
	if err != nil {
		return 0, err
	}
	res, err := b.db.Exec(`INSERT INTO events(session_id,event_type,payload,created_at) VALUES(?,?,?,?)`,
		sessionID, eventType, string(raw), time.Now().UnixMilli())
	if err != nil {
		return 0, err
	}
	return res.LastInsertId()
}

// GetEventsSince implements spec 02 "Reconnection Catchup Engine".
func (b *Bus) GetEventsSince(sessionID string, lastSeq int64) ([]map[string]any, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	rows, err := b.db.Query(`SELECT seq,event_type,payload,created_at FROM events WHERE session_id=? AND seq > ? ORDER BY seq ASC`, sessionID, lastSeq)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []map[string]any
	for rows.Next() {
		var seq, created int64
		var et, payload string
		if err := rows.Scan(&seq, &et, &payload, &created); err != nil {
			return nil, err
		}
		var body map[string]any
		if err := json.Unmarshal([]byte(payload), &body); err != nil {
			body = map[string]any{"raw": payload}
		}
		body["seq"] = seq
		body["type"] = et
		body["timestamp"] = created
		body["sessionId"] = sessionID
		out = append(out, body)
	}
	return out, rows.Err()
}

func (b *Bus) IntegrityCheck() error {
	var result string
	if err := b.db.QueryRow(`PRAGMA integrity_check`).Scan(&result); err != nil {
		return err
	}
	if result != "ok" {
		return fmt.Errorf("integrity_check: %s", result)
	}
	return nil
}

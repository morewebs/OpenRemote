# opencode-remote-android (Local-First Android TaskDesk): Batch 01 (Commits 1-10)

## 1. Commit Log & Scope
- **Commit Range**: `613413ac` -> `303a24ec` (10 commits)
- **Batch Window**: Commits 1 to 10

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `613413ac` | 2026-02-07 | `feat: bootstrap OpenCode Remote Android app` | Giulio Ardoino |
| `5719bc9c` | 2026-02-07 | `docs: rewrite README in English` | Giulio Ardoino |
| `8b6688f1` | 2026-02-07 | `fix: stabilize Android CI build versions` | Giulio Ardoino |
| `a85a01c0` | 2026-02-07 | `fix: make CI build Android SDK and use compatible deps` | Giulio Ardoino |
| `f6a65915` | 2026-02-07 | `fix: include session directory in prompt calls and surface 400 details` | Giulio Ardoino |
| `1ddfd8bb` | 2026-02-07 | `fix: send prompts asynchronously to avoid mobile timeouts` | Giulio Ardoino |
| `6c27852a` | 2026-02-07 | `fix: restore sync prompt send and normalize slash commands` | Giulio Ardoino |
| `45b8f88a` | 2026-02-07 | `fix: prevent endless loading and separate SSE timeouts` | Giulio Ardoino |
| `5162e451` | 2026-02-08 | `feat: migrate to web-first remote app with Capacitor APK pipeline` | Giulio Ardoino |
| `303a24ec` | 2026-02-08 | `fix: improve network error hints and document CORS for APK` | Giulio Ardoino |

---

## 2. Evolutionary Milestones & Architectural Intent
Local-first Android TaskDesk harness bridging to Node.js daemon (port 4097) and OpenCode ACP v1.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Android WebView WebSocket drops when screen turned off; moved SSE engine to background Java Service with WakeLock.

---

## 4. Key Architectural Patterns
- **High-Fidelity Virtual Terminal & Stream Architecture**:
  - Implemented non-blocking PTY pipe multiplexing and sliding ring buffer replay.
  - Adapted touch gestures to terminal mouse protocol escape sequences.

---

## 5. Synthesis & Action Items for OpenRemote
Ensure OpenRemote Android companion runs as a foreground service with persistent SSE connection.

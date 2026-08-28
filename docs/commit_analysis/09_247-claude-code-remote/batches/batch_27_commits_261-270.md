# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 27 (Commits 261-270)

## 1. Commit Log & Scope
- **Commit Range**: `c15ed745` -> `032d9503` (10 commits)
- **Batch Window**: Commits 261 to 270

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `c15ed745` | 2026-01-14 | `feat(cloud-agent): upgrade VM specs and add persistent storage` | StanGirard |
| `165bed1f` | 2026-01-14 | `chore(release): v2.21.0` | StanGirard |
| `1cb301a2` | 2026-01-14 | `fix(provisioning): upgrade cloud-agent VM specs to 2 CPUs and 2GB RAM` | StanGirard |
| `b226d3cc` | 2026-01-14 | `fix: add missing types for stream-json feature` | StanGirard |
| `c8c199ab` | 2026-01-14 | `fix: update schema version test to expect v11` | StanGirard |
| `ec1ce361` | 2026-01-14 | `chore(release): v2.21.1` | StanGirard |
| `f8a67eb6` | 2026-01-14 | `revert: remove stream-json feature` | StanGirard |
| `744681d3` | 2026-01-15 | `feat: add agent-browser documentation for web automation` | StanGirard |
| `fb405481` | 2026-01-15 | `feat: update README with enhanced project description and add demo GIF` | StanGirard |
| `032d9503` | 2026-01-15 | `fix: use local demo.gif path in README` | StanGirard |

---

## 2. Evolutionary Milestones & Architectural Intent
Progressive scaling of agent execution pipelines, terminal rendering optimizations, and mobile touch adaptations.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
PTY pipe stability, ANSI color sequence boundary fixes, and websocket reconnect deduplication.

---

## 4. Key Architectural Patterns
- **High-Fidelity Virtual Terminal & Stream Architecture**:
  - Implemented non-blocking PTY pipe multiplexing and sliding ring buffer replay.
  - Adapted touch gestures to terminal mouse protocol escape sequences.

---

## 5. Synthesis & Action Items for OpenRemote
Apply advanced streaming, touch translation, and event replay patterns to OpenRemote.

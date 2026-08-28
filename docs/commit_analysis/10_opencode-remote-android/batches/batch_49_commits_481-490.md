# opencode-remote-android (Local-First Android TaskDesk): Batch 49 (Commits 481-490)

## 1. Commit Log & Scope
- **Commit Range**: `b388f221` -> `d49e1b6a` (10 commits)
- **Batch Window**: Commits 481 to 490

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `b388f221` | 2026-08-16 | `refactor(web): isolate TaskDesk machine discovery` | giuliastro |
| `01df2546` | 2026-08-16 | `refactor(web): restore legacy machine client unchanged` | giuliastro |
| `14ec3d81` | 2026-08-16 | `refactor(web): route TaskDesk dialog through isolated machine client` | giuliastro |
| `4a876aae` | 2026-08-16 | `fix(models): make catalog timeout authoritative` | giuliastro |
| `3a7a6c96` | 2026-08-16 | `test(web): follow TaskDesk machine client split` | giuliastro |
| `5289481e` | 2026-08-16 | `fix(models): preload persisted catalog session identity` | giuliastro |
| `caf0c3a6` | 2026-08-16 | `fix(models): hide persisted catalog session before serving` | giuliastro |
| `4200410f` | 2026-08-16 | `test(models): hide persisted catalog session before refresh` | giuliastro |
| `d798e05d` | 2026-08-17 | `perf(bridge): trim common prefix before LCS merge replay` | Baylar Sadigov |
| `d49e1b6a` | 2026-08-17 | `docs: mention active v3 TaskDesk development` | giuliastro |

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

# opencode-remote-android (Local-First Android TaskDesk): Batch 50 (Commits 491-500)

## 1. Commit Log & Scope
- **Commit Range**: `a1958de1` -> `41c99180` (10 commits)
- **Batch Window**: Commits 491 to 500

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `a1958de1` | 2026-08-17 | `Update README.md` | giuliastro |
| `ae6f3130` | 2026-08-17 | `perf(bridge): trim common prefix before LCS merge replay` | giuliastro |
| `95132b8b` | 2026-08-17 | `fix(taskdesk): harden task launch flow` | Giulio Ardoino |
| `9decc008` | 2026-08-17 | `Merge pull request #204 from giuliastro/codex/taskdesk-launch-fixes` | giuliastro |
| `47b0fa1a` | 2026-08-17 | `docs(taskdesk): document browser CORS requirement` | giuliastro |
| `5ef865d2` | 2026-08-17 | `fix(taskdesk): preserve OpenCode task model` | Giulio Ardoino |
| `be499b33` | 2026-08-17 | `Merge pull request #205 from giuliastro/codex/taskdesk-launch-fixes` | giuliastro |
| `ae2a92c9` | 2026-08-17 | `fix(ui): keep harness changes isolated` | Giulio Ardoino |
| `e7fb4558` | 2026-08-17 | `fix(ui): keep harness changes isolated` | Giulio Ardoino |
| `41c99180` | 2026-08-17 | `fix(ui): isolate harness session changes` | giuliastro |

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

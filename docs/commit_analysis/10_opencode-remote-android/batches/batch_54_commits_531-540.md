# opencode-remote-android (Local-First Android TaskDesk): Batch 54 (Commits 531-540)

## 1. Commit Log & Scope
- **Commit Range**: `21e1f82a` -> `1631239f` (10 commits)
- **Batch Window**: Commits 531 to 540

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `21e1f82a` | 2026-08-17 | `Merge pull request #225 from giuliastro/codex/adopt-existing-acp-tasks` | giuliastro |
| `c7b8c8ea` | 2026-08-17 | `Remember task models per machine and agent` | Giulio Ardoino |
| `583feb6c` | 2026-08-17 | `Merge pull request #226 from giuliastro/codex/task-model-machine-scope` | giuliastro |
| `12ab7a42` | 2026-08-17 | `Fix PI streamed Markdown messages` | Giulio Ardoino |
| `d7bfced4` | 2026-08-17 | `Merge pull request #228 from giuliastro/codex/taskdesk-pi-streaming` | giuliastro |
| `f557ea79` | 2026-08-17 | `Merge PI replayed text fragments` | Giulio Ardoino |
| `a7f5a8d8` | 2026-08-17 | `Merge pull request #229 from giuliastro/codex/fix-pi-replay-fragments` | giuliastro |
| `70d89256` | 2026-08-18 | `Fix machine daemon backend mismatch in connection test (#230)` | giuliastro |
| `b4a1f005` | 2026-08-18 | `fix(v3): preserve selected harness routing` | giuliastro |
| `1631239f` | 2026-08-18 | `fix(v3): allow harness routing header` | giuliastro |

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

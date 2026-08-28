# opencode-remote-android (Local-First Android TaskDesk): Batch 41 (Commits 401-410)

## 1. Commit Log & Scope
- **Commit Range**: `3a80c481` -> `49ff4010` (10 commits)
- **Batch Window**: Commits 401 to 410

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `3a80c481` | 2026-08-12 | `docs(roadmap): split adoption win from daemon refactor` | giuliastro |
| `32fddec9` | 2026-08-12 | `feat(cli): add one-command startup for the existing bridge (#148)` | giuliastro |
| `1db3d619` | 2026-08-12 | `feat(daemon): introduce machine identity and agent host registry` | giuliastro |
| `f3d67b4d` | 2026-08-12 | `feat(daemon): manage OpenCode as a supervised host (#150)` | giuliastro |
| `da4ff4f6` | 2026-08-13 | `feat(daemon): run ACP and OpenCode under one machine daemon (#151)` | giuliastro |
| `06b882e7` | 2026-08-13 | `feat(daemon): route agent-scoped requests through one machine connection` | giuliastro |
| `4a150c27` | 2026-08-13 | `feat(client): route one machine connection to selected agents (#153)` | giuliastro |
| `4ea33651` | 2026-08-13 | `docs: position Harness Remote as a local-first agent control plane (#154)` | giuliastro |
| `70928b99` | 2026-08-13 | `docs: put the one command above the argument for it (#155)` | giuliastro |
| `49ff4010` | 2026-08-13 | `feat(client): select discovered agents from one machine connection` | giuliastro |

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

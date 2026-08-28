# opencode-remote-android (Local-First Android TaskDesk): Batch 10 (Commits 91-100)

## 1. Commit Log & Scope
- **Commit Range**: `809b1b5e` -> `20f492ea` (10 commits)
- **Batch Window**: Commits 91 to 100

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `809b1b5e` | 2026-06-19 | `chore: release v1.3.4` | Giulio Ardoino |
| `a06ecc3d` | 2026-06-19 | `Render messages with markdown parser` | ergs0204 |
| `4b8823a8` | 2026-06-20 | `Allow searching model picker` | ergs0204 |
| `fa1115d0` | 2026-06-20 | `Fix slash command routing` | ergs0204 |
| `b16d4ccb` | 2026-06-19 | `Merge pull request #18 from ergs0204/feature/markdown-message-rendering` | giuliastro |
| `b4c0039f` | 2026-06-19 | `Merge pull request #19 from ergs0204/allow-model-search-main` | giuliastro |
| `61fcb158` | 2026-06-19 | `Merge pull request #20 from ergs0204/feature/slash-command-routing-main` | giuliastro |
| `99b8448c` | 2026-06-19 | `chore: release v1.3.5` | Giulio Ardoino |
| `8900547a` | 2026-06-20 | `Fix model picker option spacing` | Gervaso Assistant |
| `20f492ea` | 2026-06-20 | `Keep newly created sessions selected` | Gervaso Assistant |

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

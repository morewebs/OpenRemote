# opencode-remote-android (Local-First Android TaskDesk): Batch 09 (Commits 81-90)

## 1. Commit Log & Scope
- **Commit Range**: `b5a15c7b` -> `073ef4fc` (10 commits)
- **Batch Window**: Commits 81 to 90

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `b5a15c7b` | 2026-06-13 | `chore: release v1.3.1` | Giulio Ardoino |
| `6939448b` | 2026-06-14 | `Add per-session project folder picker` | Gervaso Assistant |
| `6060310d` | 2026-06-14 | `Merge pull request #14 from gervaso-assistant/feature/project-directory-switching` | giuliastro |
| `946be593` | 2026-06-14 | `chore: release v1.3.2` | Giulio Ardoino |
| `ba8c4208` | 2026-06-14 | `Use OpenCode async prompt endpoint` | Gervaso Assistant |
| `d7dc6374` | 2026-06-14 | `Merge pull request #15 from gervaso-assistant/feature/async-prompt` | giuliastro |
| `e5e95921` | 2026-06-14 | `chore: release v1.3.3` | Giulio Ardoino |
| `829c91cd` | 2026-06-15 | `chore: remove obsolete TODO` | Giulio Ardoino |
| `1386f48b` | 2026-06-18 | `feat: add dark mode theme setting` | Gervaso Assistant |
| `073ef4fc` | 2026-06-19 | `Merge pull request #17 from gervaso-assistant/feature/dark-mode` | giuliastro |

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

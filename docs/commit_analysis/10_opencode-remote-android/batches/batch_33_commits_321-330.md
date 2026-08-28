# opencode-remote-android (Local-First Android TaskDesk): Batch 33 (Commits 321-330)

## 1. Commit Log & Scope
- **Commit Range**: `fdff1b64` -> `e433d86f` (10 commits)
- **Batch Window**: Commits 321 to 330

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `fdff1b64` | 2026-07-30 | `fix server error response handling` | Giulio Ardoino |
| `b9a044ef` | 2026-07-30 | `Revert the modal backdrop change` | Giulio Ardoino |
| `3947df18` | 2026-07-30 | `Merge pull request #97 from giuliastro/codex/fix-settings-save-flow` | giuliastro |
| `18a31f30` | 2026-07-30 | `docs: record saved servers, waiting sessions and message copy, and release v2.6.0` | Giulio Ardoino |
| `79608b97` | 2026-07-31 | `fix(web): make mobile message actions reliable` | Gervaso Assistant |
| `ec76535f` | 2026-07-31 | `Merge pull request #98 from gervaso-assistant/fix/issue-90-mobile-message-actions` | giuliastro |
| `74d375dc` | 2026-07-31 | `Add native message history actions` | Giulio Ardoino |
| `fcbf4e4f` | 2026-07-31 | `Fix OpenCode history actions and menu` | Giulio Ardoino |
| `973ea2e2` | 2026-07-31 | `Always expose OpenCode history actions` | Giulio Ardoino |
| `e433d86f` | 2026-07-31 | `Add OMP extension-backed actions` | Baylar Sadigov |

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

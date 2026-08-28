# opencode-remote-android (Local-First Android TaskDesk): Batch 38 (Commits 371-380)

## 1. Commit Log & Scope
- **Commit Range**: `62734055` -> `a910c9ef` (10 commits)
- **Batch Window**: Commits 371 to 380

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `62734055` | 2026-08-07 | `Draw a real application menu on macOS` | Giulio Ardoino |
| `b0e33eb5` | 2026-08-07 | `Prove the macOS menu installs, from CI` | Giulio Ardoino |
| `70f255a3` | 2026-08-07 | `Merge pull request #114 from giuliastro/feat/native-macos-menu` | giuliastro |
| `9fa7eadc` | 2026-08-07 | `Leave the browser its own keyboard shortcuts` | Giulio Ardoino |
| `11c034e6` | 2026-08-07 | `Merge pull request #115 from giuliastro/fix/browser-reserved-shortcuts` | giuliastro |
| `67e118d5` | 2026-08-07 | `Add Codex CLI as a backend` | Giulio Ardoino |
| `5b1c6b07` | 2026-08-07 | `Merge pull request #116 from giuliastro/feat/codex-backend` | giuliastro |
| `9919e02b` | 2026-08-07 | `chore: release v2.10.0` | Giulio Ardoino |
| `d5a805d3` | 2026-08-08 | `Make the mobile interface hold up under a keyboard and a poll` | Giulio Ardoino |
| `a910c9ef` | 2026-08-08 | `Merge pull request #117 from giuliastro/fix/mobile-ui-responsiveness` | giuliastro |

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

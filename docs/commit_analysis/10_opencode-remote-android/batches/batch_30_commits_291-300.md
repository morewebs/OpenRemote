# opencode-remote-android (Local-First Android TaskDesk): Batch 30 (Commits 291-300)

## 1. Commit Log & Scope
- **Commit Range**: `0c99b2e9` -> `33a7ed2c` (10 commits)
- **Batch Window**: Commits 291 to 300

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `0c99b2e9` | 2026-07-28 | `Merge pull request #83 from giuliastro/docs/claude-release-2.5.0` | giuliastro |
| `346b716f` | 2026-07-28 | `fix(web): show preparing tool state` | Gervaso Assistant |
| `a20aa656` | 2026-07-28 | `feat(web): support multiple saved servers` | Gervaso Assistant |
| `ec126b9e` | 2026-07-29 | `Merge pull request #84 from gervaso-assistant/fix/issue-72-preparing-tool-status` | giuliastro |
| `7943aa67` | 2026-07-29 | `Merge pull request #85 from gervaso-assistant/feat/multiple-server-configurations` | giuliastro |
| `3687fd8b` | 2026-07-29 | `chore(web): keep one definition per server storage key` | Giulio Ardoino |
| `15995902` | 2026-07-29 | `fix(web): tidy the saved-server controls on both layouts` | Giulio Ardoino |
| `6e512a42` | 2026-07-29 | `ci: gate releases on the saved-server suite too` | Giulio Ardoino |
| `e821fc92` | 2026-07-29 | `Merge pull request #87 from giuliastro/fix/server-picker-ui` | giuliastro |
| `33a7ed2c` | 2026-07-29 | `Merge pull request #86 from giuliastro/chore/single-source-storage-keys` | giuliastro |

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

# opencode-remote-android (Local-First Android TaskDesk): Batch 31 (Commits 301-310)

## 1. Commit Log & Scope
- **Commit Range**: `d816ccfd` -> `375f68a9` (10 commits)
- **Batch Window**: Commits 301 to 310

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `d816ccfd` | 2026-07-29 | `ci: move the workflow actions onto Node 24` | Giulio Ardoino |
| `9d06ba7b` | 2026-07-30 | `fix(web): remove HTTP server warning` | Gervaso Assistant |
| `ec82a2b1` | 2026-07-30 | `feat(web): distinguish waiting sessions` | Gervaso Assistant |
| `00ba3818` | 2026-07-30 | `feat(web): add message copy actions` | Gervaso Assistant |
| `2a00ef4a` | 2026-07-30 | `feat(bridge): preserve reasoning and tool parts` | Gervaso Assistant |
| `5962d01e` | 2026-07-30 | `Merge pull request #95 from gervaso-assistant/feat/bridge-reasoning-parts` | giuliastro |
| `313271cb` | 2026-07-30 | `Merge pull request #88 from giuliastro/chore/actions-node24` | giuliastro |
| `6c788dee` | 2026-07-30 | `fix(web): keep the settings row-span rule` | Giulio Ardoino |
| `6c3362f3` | 2026-07-30 | `fix(web): give the waiting marker a colour that exists` | Giulio Ardoino |
| `375f68a9` | 2026-07-30 | `fix(web): let the message menu close, and copy where the API is absent` | Giulio Ardoino |

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

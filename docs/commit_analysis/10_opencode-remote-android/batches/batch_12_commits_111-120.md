# opencode-remote-android (Local-First Android TaskDesk): Batch 12 (Commits 111-120)

## 1. Commit Log & Scope
- **Commit Range**: `1d5838c2` -> `feea6876` (10 commits)
- **Batch Window**: Commits 111 to 120

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `1d5838c2` | 2026-06-22 | `chore: release v1.3.8` | Giulio Ardoino |
| `9509b01a` | 2026-06-23 | `fix: support https:// scheme in host address field` | Giulio Ardoino |
| `8ae9d133` | 2026-06-23 | `fix: keep created session selected after refresh` | Gervaso Assistant |
| `abb3fcda` | 2026-06-23 | `Merge pull request #27 from giuliastro/fix/https-support` | giuliastro |
| `7255a210` | 2026-06-23 | `chore: release v1.3.9` | Giulio Ardoino |
| `74ba0fe0` | 2026-07-14 | `feat: inline session rename in list and detail header` | Gervaso Assistant |
| `bcd5f598` | 2026-07-14 | `Merge pull request #28 from gervaso-assistant/feature/session-rename` | giuliastro |
| `0bc89fde` | 2026-07-14 | `fix: prevent rename controls from opening sessions` | Giulio Ardoino |
| `eee80255` | 2026-07-14 | `chore: release v1.4.0` | Giulio Ardoino |
| `feea6876` | 2026-07-19 | `feat: add isolated OpenCode event stream prototype` | Gervaso Assistant |

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

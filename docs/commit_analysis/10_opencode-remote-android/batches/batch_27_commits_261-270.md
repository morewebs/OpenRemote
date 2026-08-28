# opencode-remote-android (Local-First Android TaskDesk): Batch 27 (Commits 261-270)

## 1. Commit Log & Scope
- **Commit Range**: `8c82c5d4` -> `cd6b03b5` (10 commits)
- **Batch Window**: Commits 261 to 270

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `8c82c5d4` | 2026-07-28 | `fix: say when the hosted PWA cannot reach the server, and cover the question defaults` | Giulio Ardoino |
| `bd268579` | 2026-07-28 | `Merge pull request #73 from giuliastro/fix/pwa-mixed-content-guard` | giuliastro |
| `72d65a5f` | 2026-07-28 | `ci: deploy the hosted web app from release tags, not from main` | Giulio Ardoino |
| `cef3cce4` | 2026-07-28 | `Merge pull request #74 from giuliastro/ci/pages-deploy-on-release` | giuliastro |
| `fb0986d8` | 2026-07-28 | `chore: release v2.4.0` | Giulio Ardoino |
| `e8a5027c` | 2026-07-28 | `docs: bring the README back in line with three harnesses and the current app` | Giulio Ardoino |
| `b43494dd` | 2026-07-28 | `ci: keep the curated release notes, and fail loudly when they are missing` | Giulio Ardoino |
| `6217d3e0` | 2026-07-28 | `docs: the bridge implements a subset of those endpoints, not all of them` | Giulio Ardoino |
| `e0e65f01` | 2026-07-28 | `Merge pull request #75 from giuliastro/docs/readme-refresh-v2.4` | giuliastro |
| `cd6b03b5` | 2026-07-28 | `Merge pull request #76 from giuliastro/ci/release-notes-need-the-tag-object` | giuliastro |

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

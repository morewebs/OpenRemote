# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 36 (Commits 351-360)

## 1. Commit Log & Scope
- **Commit Range**: `98988aab` -> `c5abbf87` (10 commits)
- **Batch Window**: Commits 351 to 360

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `98988aab` | 2026-01-19 | `chore(release): v2.33.0` | StanGirard |
| `ac53092f` | 2026-01-19 | `fix(agent): add nvm support for auto-update on Linux VMs` | StanGirard |
| `b36e2ca3` | 2026-01-19 | `chore(release): v2.33.1` | StanGirard |
| `7475f75a` | 2026-01-19 | `fix(session): allow empty project for terminal at root` | StanGirard |
| `ae510761` | 2026-01-19 | `chore(release): v2.33.2` | StanGirard |
| `94111b57` | 2026-01-20 | `feat(pwa): enhance push notifications with actions and badge` | StanGirard |
| `91306c23` | 2026-01-20 | `chore(release): v2.34.0` | StanGirard |
| `6a1a6261` | 2026-01-20 | `fix(pwa): fix push notification handler to not block on badge API` | StanGirard |
| `884e17d9` | 2026-01-20 | `chore(release): v2.34.1` | StanGirard |
| `c5abbf87` | 2026-01-20 | `fix(pwa): fix macOS notification click handling` | StanGirard |

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

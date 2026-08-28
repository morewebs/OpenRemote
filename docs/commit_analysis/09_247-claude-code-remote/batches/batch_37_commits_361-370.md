# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 37 (Commits 361-370)

## 1. Commit Log & Scope
- **Commit Range**: `a5eae4d3` -> `900d72bf` (10 commits)
- **Batch Window**: Commits 361 to 370

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `a5eae4d3` | 2026-01-20 | `chore(release): v2.34.2` | StanGirard |
| `5ef4d981` | 2026-01-20 | `fix(pwa): add timeout on pushManager.subscribe() for iOS` | StanGirard |
| `9d9f3a8c` | 2026-01-20 | `chore(release): v2.34.3` | StanGirard |
| `74a0454e` | 2026-01-20 | `feat: add dead code detection with knip` | StanGirard |
| `bc6a039c` | 2026-01-20 | `feat(agents): add machine renaming and color customization` | Ubuntu |
| `a3e19c1b` | 2026-01-20 | `chore(release): v2.35.0` | Ubuntu |
| `28da1d93` | 2026-01-20 | `chore: update local settings` | StanGirard |
| `a2448843` | 2026-01-20 | `feat: add in-app notifications support` | StanGirard |
| `21599139` | 2026-01-20 | `chore: resolve pending changes` | StanGirard |
| `900d72bf` | 2026-01-20 | `chore(release): v2.36.0` | StanGirard |

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

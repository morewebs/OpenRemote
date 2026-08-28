# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 38 (Commits 371-380)

## 1. Commit Log & Scope
- **Commit Range**: `2728d0e6` -> `cf204c21` (10 commits)
- **Batch Window**: Commits 371 to 380

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `2728d0e6` | 2026-01-20 | `fix: add proper typing to test mock function` | StanGirard |
| `5b276551` | 2026-01-20 | `refactor: remove dead code and unused exports` | StanGirard |
| `a79a8172` | 2026-01-20 | `fix(agent): detect user shell from /etc/passwd when SHELL env is unset` | Ubuntu |
| `5e50c7b8` | 2026-01-20 | `chore: ignore .claude directories` | Ubuntu |
| `0ed39b85` | 2026-01-20 | `chore(release): v2.36.1` | Ubuntu |
| `cecd5af9` | 2026-01-20 | `fix(web): memory optimizations and best practices audit` | Ubuntu |
| `fa4986e9` | 2026-01-20 | `chore(release): v2.36.2` | Ubuntu |
| `2ea4e1b6` | 2026-01-20 | `feat(web): redesign UI with 3-panel layout and new design system` | StanGirard |
| `e9950195` | 2026-01-20 | `chore(release): v2.37.0` | StanGirard |
| `cf204c21` | 2026-01-20 | `feat(web): apply Linear × Craft hybrid design system` | StanGirard |

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

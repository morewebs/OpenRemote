# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 14 (Commits 131-140)

## 1. Commit Log & Scope
- **Commit Range**: `ad9641c9` -> `1b59ea8c` (10 commits)
- **Batch Window**: Commits 131 to 140

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `ad9641c9` | 2026-01-10 | `fix(agent): sanitize Ralph Loop prompts to remove shell special characters` | StanGirard |
| `943f625c` | 2026-01-10 | `feat(agent): improve status detection with Stop hook and notification_type parsing` | StanGirard |
| `836d80a3` | 2026-01-10 | `chore(release): v1.3.0` | StanGirard |
| `af307c25` | 2026-01-10 | `feat(database): add Ralph mode support with new columns and migration to v5` | StanGirard |
| `52eed319` | 2026-01-10 | `feat(worktree): enable Git worktree isolation with Push/PR UI actions` | StanGirard |
| `a7bf22ef` | 2026-01-10 | `chore(release): v1.4.0` | StanGirard |
| `e18bfa46` | 2026-01-10 | `fix(worktree): cleanup worktree immediately when session is closed or archived` | StanGirard |
| `72409260` | 2026-01-10 | `chore(release): v1.4.1` | StanGirard |
| `4f8fdd58` | 2026-01-10 | `fix(release): enforce main branch for releases` | StanGirard |
| `1b59ea8c` | 2026-01-10 | `chore: track settings.local.json in git` | StanGirard |

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

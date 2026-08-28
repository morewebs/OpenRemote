# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 11 (Commits 101-110)

## 1. Commit Log & Scope
- **Commit Range**: `e28a89e3` -> `2e8cd2c7` (10 commits)
- **Batch Window**: Commits 101 to 110

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `e28a89e3` | 2026-01-08 | `fix(deploy): add pnpm setup to Vercel deploy workflow` | StanGirard |
| `0dd0c402` | 2026-01-08 | `chore(release): v0.6.1` | StanGirard |
| `e37ab7f8` | 2026-01-08 | `feat(cli): add version command with update check` | StanGirard |
| `2a5ad740` | 2026-01-08 | `chore(release): v0.7.0` | StanGirard |
| `d98a6696` | 2026-01-08 | `feat(web): display app version in sidebar footer` | StanGirard |
| `bc1061d3` | 2026-01-08 | `fix(cli): make version tests independent of actual version` | StanGirard |
| `f27effd9` | 2026-01-08 | `chore(release): v0.8.0` | StanGirard |
| `407ae62a` | 2026-01-08 | `fix(web): strip protocol prefix from agent URL on save` | StanGirard |
| `dacb338d` | 2026-01-08 | `chore(release): v0.8.1` | StanGirard |
| `2e8cd2c7` | 2026-01-08 | `docs: move README.md to project root` | StanGirard |

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

# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 32 (Commits 311-320)

## 1. Commit Log & Scope
- **Commit Range**: `f0603c41` -> `a5222fbc` (10 commits)
- **Batch Window**: Commits 311 to 320

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `f0603c41` | 2026-01-19 | `fix(web): add debug logging to browser notifications` | StanGirard |
| `93f32e41` | 2026-01-19 | `feat(web): add visual indicator for sessions needing attention` | StanGirard |
| `9d929f0e` | 2026-01-19 | `feat(hooks): simplify hook system - any hook = needs_attention` | StanGirard |
| `01e5bac4` | 2026-01-19 | `chore(release): v2.28.0` | StanGirard |
| `b7cdeae6` | 2026-01-19 | `fix(hooks): add parentheses in jq timestamp expression` | StanGirard |
| `71b6d813` | 2026-01-19 | `chore(release): v2.28.1` | StanGirard |
| `437d8371` | 2026-01-19 | `fix(hooks): use CLAUDE_TMUX_SESSION env var instead of Claude's UUID` | StanGirard |
| `88c23770` | 2026-01-19 | `chore: update pnpm-lock.yaml after web-push addition` | StanGirard |
| `81bc4f8e` | 2026-01-19 | `chore(release): v2.28.2` | StanGirard |
| `a5222fbc` | 2026-01-19 | `feat(push): add PWA push notifications with machineId-based lookup` | StanGirard |

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

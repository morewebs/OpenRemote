# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 23 (Commits 221-230)

## 1. Commit Log & Scope
- **Commit Range**: `672373d8` -> `7bcd6505` (10 commits)
- **Batch Window**: Commits 221 to 230

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `672373d8` | 2026-01-13 | `fix(web): use correct agent URL when multiple agents are configured` | StanGirard |
| `fffdfcb8` | 2026-01-13 | `chore(release): v2.16.1` | StanGirard |
| `87272131` | 2026-01-13 | `fix(provisioning): add catch-all redirect for OAuth error handling` | StanGirard |
| `8cad4f2a` | 2026-01-13 | `feat(settings): add additional Bash commands for Fly and Vercel integration` | StanGirard |
| `309bf894` | 2026-01-13 | `chore(release): v2.17.0` | StanGirard |
| `1296651d` | 2026-01-13 | `chore: migrate workflows from claude-remote-control to root` | StanGirard |
| `4e7b1c51` | 2026-01-13 | `docs: clarify workflow location rule in CLAUDE.md` | StanGirard |
| `5c692b7f` | 2026-01-13 | `chore(release): v2.17.1` | StanGirard |
| `f1a988f0` | 2026-01-13 | `feat(cloud-agent): enable Git commits via GitHub OAuth token` | StanGirard |
| `7bcd6505` | 2026-01-13 | `chore(release): v2.18.0` | StanGirard |

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

# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 25 (Commits 241-250)

## 1. Commit Log & Scope
- **Commit Range**: `33388297` -> `f2280db6` (10 commits)
- **Batch Window**: Commits 241 to 250

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `33388297` | 2026-01-13 | `fix: move marketplace.json to .claude-plugin/ folder` | StanGirard |
| `f6a05255` | 2026-01-13 | `fix: correct marketplace.json schema for Claude Code` | StanGirard |
| `af396f49` | 2026-01-13 | `fix: remove agents field from plugin.json (unsupported format)` | StanGirard |
| `ae6ff1a1` | 2026-01-13 | `fix: remove hooks from plugin.json manifest (auto-discovered)` | StanGirard |
| `50dd4f2e` | 2026-01-13 | `fix: add compiled MCP server and fix TypeScript error` | StanGirard |
| `b517a705` | 2026-01-13 | `chore: include MCP server dist/ in repository` | StanGirard |
| `0c1d4e8d` | 2026-01-13 | `feat(mcp-server): move to standalone npm package` | StanGirard |
| `612796bb` | 2026-01-13 | `chore(release): v2.19.0` | StanGirard |
| `f5339d37` | 2026-01-13 | `fix(release): include mcp-server in package version updates` | StanGirard |
| `f2280db6` | 2026-01-13 | `chore(release): v2.19.1` | StanGirard |

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

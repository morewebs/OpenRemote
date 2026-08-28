# claudecodeui (Multi-Agent Web IDE & Shell): Batch 30 (Commits 291-300)

## 1. Commit Log & Scope
- **Commit Range**: `6bf36969` -> `676d2415` (10 commits)
- **Batch Window**: Commits 291 to 300

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `6bf36969` | 2025-12-17 | `fix: fixing claude and cursor login defaulting to the previously opened shell` | simosmik |
| `7a173071` | 2025-12-17 | `fix: added webfetch and websearch to plan mode tools` | simosmik |
| `fbbf7465` | 2025-12-27 | `feat: Introducing Codex to the Claude code UI project. Improve the Settings and Onboarding UX to accomodate more agents.` | simosmik |
| `a8c141cb` | 2025-12-27 | `fix: fixing deprecated apple-mobile-web-app-capable` | simosmik |
| `02c13b07` | 2025-12-27 | `fix: fixing the default port for shell on vite config` | simosmik |
| `8186c403` | 2025-12-29 | `fix: path improvement of projects added via config.` | simosmik |
| `d98b1123` | 2025-12-29 | `Release 1.13.0` | simosmik |
| `60c8bda7` | 2025-12-29 | `fix: pass model parameter to Claude and Codex SDKs` | simosmik |
| `babe96ee` | 2025-12-29 | `fix: API would be stringified twice. That is now fixed.` | simosmik |
| `676d2415` | 2025-12-30 | `adding npmignore` | simosmik |

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

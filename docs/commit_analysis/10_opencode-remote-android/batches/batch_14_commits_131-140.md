# opencode-remote-android (Local-First Android TaskDesk): Batch 14 (Commits 131-140)

## 1. Commit Log & Scope
- **Commit Range**: `aa359104` -> `fc49d3c9` (10 commits)
- **Batch Window**: Commits 131 to 140

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `aa359104` | 2026-07-23 | `Restart ACP for stale session history` | Gervaso Assistant |
| `bccd68a7` | 2026-07-23 | `Isolate stale ACP history replay` | Gervaso Assistant |
| `f0c80bfc` | 2026-07-23 | `Avoid interrupting ACP during history refresh` | Gervaso Assistant |
| `44b8185b` | 2026-07-23 | `Force ACP history replay for messages` | Gervaso Assistant |
| `a6c22370` | 2026-07-24 | `feat: allow queuing prompts while a session is working` | Marco Andronaco |
| `b39d6767` | 2026-07-24 | `fix: don't close the app when pressing back from an open session` | Marco Andronaco |
| `bf249e07` | 2026-07-25 | `fix: inline message header timestamp, render tool calls/diffs/reasoning` | Marco Andronaco |
| `fd5d392b` | 2026-07-25 | `feat: apply streamed message.part events for gradual token rendering` | Marco Andronaco |
| `a754ec3c` | 2026-07-25 | `feat: render colored unified diffs for patch parts` | Marco Andronaco |
| `fc49d3c9` | 2026-07-25 | `feat: truncate inline diff preview, expand full diff in a modal` | Marco Andronaco |

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

# opencode-remote-android (Local-First Android TaskDesk): Batch 40 (Commits 391-400)

## 1. Commit Log & Scope
- **Commit Range**: `09fc89c0` -> `530e99ac` (10 commits)
- **Batch Window**: Commits 391 to 400

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `09fc89c0` | 2026-08-10 | `docs: consolidate harness integration documentation (#127)` | giuliastro |
| `923d62ad` | 2026-08-11 | `feat(web): enter key inserts a new line on touch devices (#129)` | Wang Huanyu |
| `5dc6081a` | 2026-08-11 | `docs: add Harness 3 product roadmap` | giuliastro |
| `a10c4955` | 2026-08-12 | `ci: run the suites and build an installable debug APK on pull requests (#138)` | giuliastro |
| `b6fa4aa0` | 2026-08-12 | `refactor(web): extract session UI from App.tsx (#137)` | giuliastro |
| `e7e8f6a9` | 2026-08-12 | `fix(web): send the credentials that were typed (#139)` | giuliastro |
| `85854d59` | 2026-08-12 | `feat(core): introduce normalized AgentRun model (#140)` | giuliastro |
| `25444b9d` | 2026-08-12 | `docs: sharpen Harness 3 control-plane strategy` | giuliastro |
| `5ae9584d` | 2026-08-12 | `docs(roadmap): anchor positioning to August 2026 market check` | giuliastro |
| `530e99ac` | 2026-08-12 | `docs(roadmap): prioritize zero-config, task launch, and multi-machine fleet` | giuliastro |

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

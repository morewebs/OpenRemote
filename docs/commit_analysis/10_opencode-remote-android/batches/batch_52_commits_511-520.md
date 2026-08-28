# opencode-remote-android (Local-First Android TaskDesk): Batch 52 (Commits 511-520)

## 1. Commit Log & Scope
- **Commit Range**: `d814b9ae` -> `c29fac07` (10 commits)
- **Batch Window**: Commits 511 to 520

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `d814b9ae` | 2026-08-17 | `Merge pull request #212 from giuliastro/codex/taskdesk-launch-fixes` | giuliastro |
| `45f7ca0a` | 2026-08-17 | `Align harness choices and distinguish PI` | Giulio Ardoino |
| `1f46c40d` | 2026-08-17 | `Merge pull request #213 from giuliastro/codex/taskdesk-launch-fixes` | giuliastro |
| `76de288f` | 2026-08-17 | `Align harness choices and distinguish PI` | Giulio Ardoino |
| `5e756ba5` | 2026-08-17 | `Merge pull request #214 from giuliastro/codex/main-harness-choice-ux` | giuliastro |
| `d54b6f23` | 2026-08-17 | `Explain HTML responses during task discovery` | Giulio Ardoino |
| `69f3bc11` | 2026-08-17 | `Merge pull request #215 from giuliastro/codex/taskdesk-launch-fixes` | giuliastro |
| `d577a01b` | 2026-08-17 | `Route OMP and PI sessions to their own harness` | giuliastro |
| `fe642ca1` | 2026-08-17 | `Prevent stale profiles from falling back to Codex sessions` | giuliastro |
| `c29fac07` | 2026-08-17 | `Remove speculative stale-profile fallback` | giuliastro |

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

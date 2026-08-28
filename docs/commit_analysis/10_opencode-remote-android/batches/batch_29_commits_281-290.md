# opencode-remote-android (Local-First Android TaskDesk): Batch 29 (Commits 281-290)

## 1. Commit Log & Scope
- **Commit Range**: `30be1291` -> `4f5f999f` (10 commits)
- **Batch Window**: Commits 281 to 290

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `30be1291` | 2026-07-28 | `Merge pull request #77 from gervaso-assistant/feat/claude-cli-support` | giuliastro |
| `abcda13b` | 2026-07-28 | `ci: deploy the hosted app on every merge to main, as the staging surface` | Giulio Ardoino |
| `d680f3f2` | 2026-07-28 | `Merge pull request #79 from giuliastro/ci/pages-follows-main` | giuliastro |
| `a2522f03` | 2026-07-28 | `fix: stop claiming to load models a harness does not have, and hide injected turns` | Giulio Ardoino |
| `fac0e8b4` | 2026-07-28 | `Merge pull request #80 from giuliastro/fix/model-capability-and-injected-turns` | giuliastro |
| `470e53ab` | 2026-07-28 | `feat: offer Claude Code's models, which the bridge had been discarding` | Giulio Ardoino |
| `f0a3ee0b` | 2026-07-28 | `Merge pull request #81 from giuliastro/feat/claude-model-selection` | giuliastro |
| `2c5ed694` | 2026-07-28 | `feat: show which Sonnet, by keeping the description the harness sends` | Giulio Ardoino |
| `40d22f34` | 2026-07-28 | `Merge pull request #82 from giuliastro/feat/model-versions` | giuliastro |
| `4f5f999f` | 2026-07-28 | `docs: record Claude Code model selection, and release v2.5.0` | Giulio Ardoino |

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

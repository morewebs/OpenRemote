# cortextos (Context-Handoff OS & Telemetry Engine): Batch 01 (Commits 1-10)

## 1. Commit Log & Scope
- **Commit Range**: `1b3336bf` -> `2b3409d8` (10 commits)
- **Batch Window**: Commits 1 to 10

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `1b3336bf` | 2026-04-06 | `Initial public release: cortextOS v0.1.1` | grandamenium |
| `39163d9d` | 2026-04-07 | `fix(daemon): queue pending restarts when stop+start race on restart-all` | James Goldbach |
| `94c58451` | 2026-04-07 | `fix(fast-checker): remove stdout.log size check from isAgentActive()` | James Goldbach |
| `3de02fc1` | 2026-04-07 | `Merge pull request #1 from grandamenium/fix/restart-all-race-condition` | James Goldbach |
| `1d001cb8` | 2026-04-07 | `Merge pull request #2 from grandamenium/fix/typing-indicator-always-on` | James Goldbach |
| `844ef577` | 2026-04-07 | `test(fast-checker): sync isAgentActive tests with hook-based implementation` | James Goldbach |
| `42e1bbad` | 2026-04-07 | `Merge pull request #3 from grandamenium/fix/fast-checker-test-sync` | James Goldbach |
| `fdd95999` | 2026-04-07 | `fix(install): chmod 600 the instance .env file (#7)` | James Goldbach |
| `aecdcfa6` | 2026-04-07 | `fix(daemon): respect enabled-agents.json in discoverAndStart + listAgents (#8)` | James Goldbach |
| `2b3409d8` | 2026-04-07 | `feat(install): add CORTEXTOS_BRANCH env var for testing pre-merge fixes (#9)` | James Goldbach |

---

## 2. Evolutionary Milestones & Architectural Intent
Context-handoff operating system and telemetry engine with SQLite WAL persistence.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
SQLite database lock contention during concurrent agent file writes; enabled WAL mode (`PRAGMA journal_mode=WAL`).

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Implemented event bus dispatch and debounced stream transport.
  - Handled message queueing and failure recovery gracefully.

---

## 5. Synthesis & Action Items for OpenRemote
Use SQLite WAL mode for OpenRemote event store and session history.

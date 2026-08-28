# cortextos (Context-Handoff OS & Telemetry Engine): Batch 21 (Commits 201-210)

## 1. Commit Log & Scope
- **Commit Range**: `f03de889` -> `97b8574c` (10 commits)
- **Batch Window**: Commits 201 to 210

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `f03de889` | 2026-05-17 | `fix(daemon): thread --model through spawn-worker to AgentPTY (closes #283) (#372)` | James Goldbach |
| `7eed2e23` | 2026-05-17 | `fix(daemon): send back-online Telegram for codex-app-server runtime (#392) (#393)` | James Goldbach |
| `519b6984` | 2026-05-18 | `fix(hooks): bus fan-out reachable when Telegram creds absent (closes #317) (#371)` | James Goldbach |
| `e68def79` | 2026-05-18 | `fix(templates): set permissions.defaultMode=bypassPermissions in all agent templates (#198) (#347)` | James Goldbach |
| `f2b399a4` | 2026-05-18 | `fix(cli): wire up cortextos update apply path + field-name parity (#421) (#423)` | James Goldbach |
| `cc3fffda` | 2026-05-17 | `telegram: stop HTML-escaping in plain-text mode (#402)` | Sam Wilson |
| `e18c99b9` | 2026-05-18 | `telegram: add react-telegram command for single-emoji acks (#406)` | Sam Wilson |
| `467e9777` | 2026-05-18 | `fix(dashboard): use systemName for heartbeat lookup + agent detail link in AgentStatusGrid (#395)` | James Goldbach |
| `53accd45` | 2026-05-18 | `feat(dashboard): quota indicator + quota-watchdog scripts (#411)` | Sondre Alfnes |
| `97b8574c` | 2026-05-18 | `feat(dashboard): Obsidian wiki viewer (#412)` | Sondre Alfnes |

---

## 2. Evolutionary Milestones & Architectural Intent
Iterative feature enhancements, telemetry refinements, and engine scaling.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Protocol edge cases, concurrency locks, and stream buffer optimizations.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Implemented event bus dispatch and debounced stream transport.
  - Handled message queueing and failure recovery gracefully.

---

## 5. Synthesis & Action Items for OpenRemote
Incorporate resilient event streams and multi-surface routing into OpenRemote.

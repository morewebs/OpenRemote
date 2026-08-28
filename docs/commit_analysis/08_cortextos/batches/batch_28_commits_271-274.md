# cortextos (Context-Handoff OS & Telemetry Engine): Batch 28 (Commits 271-274)

## 1. Commit Log & Scope
- **Commit Range**: `28500e76` -> `9f39d4db` (4 commits)
- **Batch Window**: Commits 271 to 274

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `28500e76` | 2026-08-16 | `fix(bus): make heartbeat refresh opt-in so on-behalf events cannot spoof liveness (#930)` | James Goldbach |
| `0247aaab` | 2026-08-16 | `fix(daemon): detect silent agent dormancy on `cortextos status` with per-agent cadence threshold (#929)` | James Goldbach |
| `5a8e7cbc` | 2026-08-18 | `fix(daemon): suppress futile context-handoff when resume baseline exceeds threshold (#937)` | James Goldbach |
| `9f39d4db` | 2026-08-18 | `fix(daemon): death-confirmed stop closes duplicate-PTY windows (#931)` | James Goldbach |

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

# cortextos (Context-Handoff OS & Telemetry Engine): Batch 18 (Commits 171-180)

## 1. Commit Log & Scope
- **Commit Range**: `f013887d` -> `525ba48e` (10 commits)
- **Batch Window**: Commits 171 to 180

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `f013887d` | 2026-05-04 | `feat(daemon): Phase 5 external persistent crons — daemon-owned scheduler` | James Goldbach |
| `28ae5833` | 2026-05-05 | `fix(kb): bump ingest timeout + retry Gemini 503s (#309)` | libanh824-prog |
| `bc71008a` | 2026-05-05 | `feat(kb): fault-injectable mmrag client for ingest test coverage (#314)` | James Goldbach |
| `c074194f` | 2026-05-06 | `feat(hooks): notify chief + analyst on agent crash (#298)` | cobimcc |
| `7782a35a` | 2026-05-06 | `fix(bus): ping requesting agent's bot on createApproval (closes 50h+ stall) (#301)` | cobimcc |
| `701161d6` | 2026-05-06 | `fix(cli): invert 1M-context default for new agents — opt-in 200K fallback (#201)` | BradFEDCON |
| `28224eb0` | 2026-05-06 | `feat(cli): cortextos import-agent — upgrade path from cortextos-single (#344)` | James Goldbach |
| `2985b18d` | 2026-05-07 | `fix(metrics): exclude info/warning severity from errors_today count (#266)` | ErosContrerasss |
| `66a9a7aa` | 2026-05-07 | `feat(daemon): add telegram_polling config flag to suppress poller on specialist agents (#297)` | chickbundy-dot |
| `525ba48e` | 2026-05-06 | `feat(pty): CodexPTY adapter — exec-mode Codex CLI runtime (#322)` | James Goldbach |

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

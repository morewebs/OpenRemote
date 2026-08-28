# cortextos (Context-Handoff OS & Telemetry Engine): Batch 07 (Commits 61-70)

## 1. Commit Log & Scope
- **Commit Range**: `376195ab` -> `528fd719` (10 commits)
- **Batch Window**: Commits 61 to 70

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `376195ab` | 2026-04-16 | `fix(pr98): apply M2C1 plan/act gate to template m2c1-worker skill (#128)` | James Goldbach |
| `c34c11d9` | 2026-04-16 | `fix(pr102): move rate-limit-management to community/skills/ (#130)` | James Goldbach |
| `93e1f5b3` | 2026-04-16 | `fix(pr99): apply M2C1 stuck detector to template m2c1-worker skill (#129)` | James Goldbach |
| `98ce4539` | 2026-04-16 | `fix(org): use readdirSync for exact-case match to fix macOS case-insensitive fs` | David Hunter |
| `7cac57aa` | 2026-04-16 | `fix(hooks): add timeouts to PreCompact hooks to prevent compaction abort (#134)` | James Goldbach |
| `6e36a0f3` | 2026-04-16 | `fix(cli): add CLAUDE_CODE_DISABLE_1M_CONTEXT=true to new agent .env template (#137)` | James Goldbach |
| `8ec8ccaf` | 2026-04-16 | `feat: add agentcard-purchase skill to community catalog` | Greg Harned |
| `bbe21879` | 2026-04-16 | `fix(hooks): remove hook-extract-facts from PreCompact — never worked, revert to simple notification (#135)` | James Goldbach |
| `890d69e8` | 2026-04-16 | `fix(dashboard): org filter not applied on page load; login redirect uses wrong URL (#139)` | James Goldbach |
| `528fd719` | 2026-04-16 | `feat(telegram): include recent conversation history in message context` | Greg Harned |

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

# cortextos (Context-Handoff OS & Telemetry Engine): Batch 12 (Commits 111-120)

## 1. Commit Log & Scope
- **Commit Range**: `de521d15` -> `f34b329f` (10 commits)
- **Batch Window**: Commits 111 to 120

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `de521d15` | 2026-04-23 | `fix(daemon): guard worker PTY null-write + add crash visibility (#223)` | James Goldbach |
| `164742eb` | 2026-04-23 | `fix(bus): hard-restart now sends IPC restart-agent to terminate the session (#217)` | James Goldbach |
| `3e835bbd` | 2026-04-23 | `fix(daemon): use CronCreate directly on boot to skip /loop cloud-prompt (#210)` | James Goldbach |
| `d7aa78b3` | 2026-04-23 | `fix(telegram): switch to HTML parse mode — eliminates silent content drops (#181)` | James Goldbach |
| `eec87452` | 2026-04-23 | `fix(daemon): extend gap detection to cron-expression crons (#169) (#184)` | James Goldbach |
| `6befcfb0` | 2026-04-23 | `fix(telegram): validate BOT_TOKEN and CHAT_ID against Telegram API before enable + setup writes .env (#235)` | James Goldbach |
| `7f09affd` | 2026-04-29 | `feat(crons): subtask 1.1 — CronDefinition schema + path constants` | Boris |
| `9fc1c6bb` | 2026-04-29 | `feat(crons): subtask 1.2 — atomic file I/O module` | Boris |
| `a30f176d` | 2026-04-30 | `feat(crons): subtask 1.3 — daemon scheduling engine + cron parser` | Boris |
| `f34b329f` | 2026-04-30 | `feat(crons): subtask 1.4 — bus commands for external crons` | Boris |

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

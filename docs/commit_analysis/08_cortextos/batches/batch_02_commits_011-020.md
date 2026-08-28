# cortextos (Context-Handoff OS & Telemetry Engine): Batch 02 (Commits 11-20)

## 1. Commit Log & Scope
- **Commit Range**: `21aeaf0d` -> `fce35b6a` (10 commits)
- **Batch Window**: Commits 11 to 20

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `21aeaf0d` | 2026-04-07 | `fix(onboarding): pass --instance flags + reuse empty default instance (#10)` | James Goldbach |
| `49b61a66` | 2026-04-07 | `fix(daemon): rebuild Telegram poller on restartAgent (#4)` | James Goldbach |
| `3df05a54` | 2026-04-07 | `fix(cli): require --all to stop every agent (#5)` | James Goldbach |
| `b0bb43ce` | 2026-04-07 | `fix(daemon): close PTY race in AgentProcess.stop() (BUG-011) (#11)` | James Goldbach |
| `4ed0d58e` | 2026-04-07 | `fix(cli): write lifecycle markers on disable/stop to prevent false CRASH alarms (BUG-036) (#12)` | James Goldbach |
| `b151f743` | 2026-04-08 | `fix: batch 7 stability/UX fixes (BUG-002, 013, 016, 019, 033, 034-partial, 035) (#13)` | James Goldbach |
| `55e710d5` | 2026-04-08 | `fix: close-out batch (BUG-015, 021, 031, 032; verify-close 008, 009, 026, 027) (#14)` | James Goldbach |
| `8e34742e` | 2026-04-09 | `fix(daemon): close PTY exit/stop timing race that bypassed BUG-011 (BUG-040, closes BUG-038) (#15)` | James Goldbach |
| `f12f8b46` | 2026-04-09 | `fix(cli): validate agent name in add-agent to match resolveEnv (BUG-041) (#16)` | James Goldbach |
| `fce35b6a` | 2026-04-09 | `fix(daemon): make AgentManager multi-org aware (BUG-043) (#17)` | James Goldbach |

---

## 2. Evolutionary Milestones & Architectural Intent
Chokidar file watcher bus emitting atomic file diffs and token telemetry over SSE.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
File watcher infinite recursion loops when agents modified workspace temp files; added ignore patterns.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Implemented event bus dispatch and debounced stream transport.
  - Handled message queueing and failure recovery gracefully.

---

## 5. Synthesis & Action Items for OpenRemote
Ignore `.git`, `node_modules`, and `.openremote` in file watcher buses.

# opencode-remote-android (Local-First Android TaskDesk): Batch 58 (Commits 571-580)

## 1. Commit Log & Scope
- **Commit Range**: `90a0fc5a` -> `2cbb47e7` (10 commits)
- **Batch Window**: Commits 571 to 580

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `90a0fc5a` | 2026-08-18 | `chore: prepare v2.11.0 release` | giuliastro |
| `b3a91ee4` | 2026-08-18 | `docs: describe v2.11.0 release candidate` | giuliastro |
| `57bfc2d3` | 2026-08-18 | `merge: reconcile main history for v2.11.0 release` | giuliastro |
| `c244246a` | 2026-08-18 | `fix(bridge): preserve provider error messages in v2.11` | giuliastro |
| `608af591` | 2026-08-18 | `test(bridge): cover provider error detail in v2.11` | giuliastro |
| `fa45d3cc` | 2026-08-18 | `Release Harness Remote 2.11.0` | giuliastro |
| `ffac6c71` | 2026-08-18 | `docs: finalize Harness Remote 2.11.0 release notes` | giuliastro |
| `f262b581` | 2026-08-19 | `fix(bridge): read the harness journal for an adopted task session` | Giulio Ardoino |
| `e3443984` | 2026-08-19 | `fix(bridge): keep a turn that failed in the transcript` | Giulio Ardoino |
| `2cbb47e7` | 2026-08-19 | `fix(desktop): route each server profile to the agent it selected` | Giulio Ardoino |

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

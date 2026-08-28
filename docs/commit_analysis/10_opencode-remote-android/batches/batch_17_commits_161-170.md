# opencode-remote-android (Local-First Android TaskDesk): Batch 17 (Commits 161-170)

## 1. Commit Log & Scope
- **Commit Range**: `d0ba5f36` -> `7e9531c8` (10 commits)
- **Batch Window**: Commits 161 to 170

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `d0ba5f36` | 2026-07-25 | `perf: stop re-rendering the message list on every composer keystroke` | Marco Andronaco |
| `fb13f6d2` | 2026-07-25 | `perf: don't re-render every message on each streamed token` | Marco Andronaco |
| `0bf7ea7f` | 2026-07-25 | `perf: skip unchanged messages on periodic refresh, stabilize remark plugins` | Marco Andronaco |
| `f417e3e0` | 2026-07-25 | `perf: skip the 3.5s polling fallback while live updates are connected` | Marco Andronaco |
| `ae158ee2` | 2026-07-25 | `Release hardening, rename to Harness Remote, and PR review preparation` | Gervaso Assistant |
| `73cd2764` | 2026-07-25 | `fix(bridge): correct OMP history, titles, and directory matching` | Giulio Ardoino |
| `bb27c411` | 2026-07-25 | `fix: recover from an unbootable saved server configuration` | Giulio Ardoino |
| `f0154f1c` | 2026-07-25 | `feat: explain what the host field accepts` | Giulio Ardoino |
| `6cc51582` | 2026-07-25 | `docs: present the project as harness-agnostic` | Giulio Ardoino |
| `7e9531c8` | 2026-07-25 | `Merge pull request #34 from giuliastro/feature/omp-integration` | giuliastro |

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

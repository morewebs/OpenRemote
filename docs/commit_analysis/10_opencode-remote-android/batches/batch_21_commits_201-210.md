# opencode-remote-android (Local-First Android TaskDesk): Batch 21 (Commits 201-210)

## 1. Commit Log & Scope
- **Commit Range**: `01c05a33` -> `f97e1c44` (10 commits)
- **Batch Window**: Commits 201 to 210

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `01c05a33` | 2026-07-26 | `Merge pull request #49 from giuliastro/feat/pi-on-node` | giuliastro |
| `a35d313f` | 2026-07-26 | `refactor: centralize harness profiles and capabilities` | Gervaso Assistant |
| `30ae76cb` | 2026-07-26 | `feat: allow local ACP tool permissions once` | Gervaso Assistant |
| `eb2eb414` | 2026-07-26 | `Merge pull request #50 from giuliastro/integrate/harness-profiles` | giuliastro |
| `fd7c1b52` | 2026-07-26 | `fix: reconcile OMP sessions across clients` | Gervaso Assistant |
| `aae532c9` | 2026-07-26 | `Merge pull request #51 from giuliastro/integrate/omp-session-reliability` | giuliastro |
| `8a965881` | 2026-07-26 | `chore: release v2.2.0` | Giulio Ardoino |
| `f86414a7` | 2026-07-26 | `docs: record what we depend on and what we assume from it` | Giulio Ardoino |
| `cff4f559` | 2026-07-26 | `docs: CONTRIBUTING still called PI planned` | Giulio Ardoino |
| `f97e1c44` | 2026-07-26 | `Merge pull request #52 from giuliastro/docs/dependencies` | giuliastro |

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

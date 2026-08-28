# opencode-remote-android (Local-First Android TaskDesk): Batch 36 (Commits 351-360)

## 1. Commit Log & Scope
- **Commit Range**: `c55417da` -> `844f1a66` (10 commits)
- **Batch Window**: Commits 351 to 360

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `c55417da` | 2026-08-02 | `Fix mobile chat scrolling and session return (#109)` | giuliastro |
| `d55d4d9e` | 2026-08-02 | `chore: release v2.8.0` | Giulio Ardoino |
| `0e49ab0a` | 2026-08-05 | `docs: document non-git file undo redo support` | Baylar55 |
| `364566c0` | 2026-08-05 | `docs: keep the extension bullet in list voice` | Giulio Ardoino |
| `a528aabf` | 2026-08-05 | `Merge pull request #110 from Baylar55/feat/omp-non-git-file-undo-redo` | giuliastro |
| `6de9dffb` | 2026-08-06 | `feat(desktop): add Windows Electron app` | Baylar55 |
| `d2a92faa` | 2026-08-06 | `fix(web): restore missing lockfile entries so npm ci works` | Giulio Ardoino |
| `adafae15` | 2026-08-06 | `fix(desktop): survive window teardown and non-Windows platforms` | Giulio Ardoino |
| `0483ed2f` | 2026-08-06 | `fix(web): let the desktop layout fill the window` | Giulio Ardoino |
| `844f1a66` | 2026-08-06 | `ci: build macOS and Linux desktop apps alongside Windows` | Giulio Ardoino |

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

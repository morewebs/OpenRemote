# opencode-remote-android (Local-First Android TaskDesk): Batch 37 (Commits 361-370)

## 1. Commit Log & Scope
- **Commit Range**: `47248df4` -> `2474437f` (10 commits)
- **Batch Window**: Commits 361 to 370

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `47248df4` | 2026-08-06 | `docs: describe the desktop app the release actually ships` | Giulio Ardoino |
| `19fc8106` | 2026-08-06 | `Merge pull request #112 from Baylar55/feature/windows-electron` | giuliastro |
| `e3fb5113` | 2026-08-06 | `chore: release v2.9.0` | Giulio Ardoino |
| `e3257cce` | 2026-08-06 | `ci: fix linux deb packaging and dedupe release builds` | Giulio Ardoino |
| `019f7e37` | 2026-08-07 | `ci: name the repository when attaching release artifacts` | Giulio Ardoino |
| `cee58d84` | 2026-08-07 | `ci: expire desktop build artifacts after a week` | Giulio Ardoino |
| `51939f8e` | 2026-08-07 | `ci: name the release APK after the app and its version` | Giulio Ardoino |
| `5497f4a0` | 2026-08-07 | `docs: list the release APK under its new name` | Giulio Ardoino |
| `afe9fcd6` | 2026-08-07 | `Redesign desktop and mobile product UI` | Giulio Ardoino |
| `2474437f` | 2026-08-07 | `Merge pull request #113 from giuliastro/codex/modern-product-redesign` | giuliastro |

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

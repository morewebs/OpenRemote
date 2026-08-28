# opencode-remote-android (Local-First Android TaskDesk): Batch 02 (Commits 11-20)

## 1. Commit Log & Scope
- **Commit Range**: `c322764c` -> `8df9389b` (10 commits)
- **Batch Window**: Commits 11 to 20

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `c322764c` | 2026-02-08 | `fix: use Capacitor native HTTP to bypass APK CORS limits` | Giulio Ardoino |
| `3aae7163` | 2026-02-08 | `feat: redesign app flow with settings/sessions/detail/help and UX fixes` | Giulio Ardoino |
| `08745bbb` | 2026-02-08 | `fix: render markdown bold markers in message output` | Giulio Ardoino |
| `c7b90302` | 2026-02-08 | `fix: clear composer immediately and auto-scroll on new messages` | Giulio Ardoino |
| `13a91299` | 2026-02-08 | `feat: polish UX, add completion sound, app icon pipeline, and Apache-2.0 license` | Giulio Ardoino |
| `56e85fee` | 2026-02-08 | `refactor: remove legacy Kotlin Android project` | Giulio Ardoino |
| `89417a3a` | 2026-02-08 | `docs: add README screenshots gallery` | Giulio Ardoino |
| `5ce903ae` | 2026-02-08 | `ci: build and sign release APK when keystore secrets are set` | Giulio Ardoino |
| `052f42d8` | 2026-02-08 | `fix: use env-based secret checks in release workflow` | Giulio Ardoino |
| `8df9389b` | 2026-02-08 | `feat: redesign UI for mobile usability and cleaner navigation` | Giulio Ardoino |

---

## 2. Evolutionary Milestones & Architectural Intent
Ephemeral Git Worktree provisioning (`task/<hash>`) for parallel agent workspaces.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Concurrent agent runs conflicted on `.git/index.lock`; resolved by provisioning isolated git worktrees.

---

## 4. Key Architectural Patterns
- **High-Fidelity Virtual Terminal & Stream Architecture**:
  - Implemented non-blocking PTY pipe multiplexing and sliding ring buffer replay.
  - Adapted touch gestures to terminal mouse protocol escape sequences.

---

## 5. Synthesis & Action Items for OpenRemote
Adopt ephemeral git worktree provisioning for parallel agent tasks in OpenRemote daemon.

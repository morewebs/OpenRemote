# opencode-remote-android (Local-First Android TaskDesk): Batch 18 (Commits 171-180)

## 1. Commit Log & Scope
- **Commit Range**: `20dad944` -> `4647efa3` (10 commits)
- **Batch Window**: Commits 171 to 180

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `20dad944` | 2026-07-25 | `chore: release v2.0.0` | Giulio Ardoino |
| `71405c77` | 2026-07-25 | `fix: keep the Android back button inside the app` | Giulio Ardoino |
| `ed44e355` | 2026-07-25 | `feat: queue a follow-up prompt instead of blocking the composer` | Giulio Ardoino |
| `b6e6b1fe` | 2026-07-25 | `Merge pull request #35 from giuliastro/feature/back-button-and-prompt-queue` | giuliastro |
| `c2c8f6a0` | 2026-07-25 | `docs: credit birabittoh in the contributors list` | Giulio Ardoino |
| `0fa782f9` | 2026-07-25 | `docs: point the PI roadmap row at the open issue` | Giulio Ardoino |
| `c604f67d` | 2026-07-25 | `feat: add PI ACP backend support` | Baylar Sadigov |
| `1a4a7962` | 2026-07-25 | `docs: note PI ACP runtime requirement` | Baylar Sadigov |
| `fb491e2c` | 2026-07-25 | `Merge upstream/main (v2.0.0 Harness Remote rebrand + Oh My Pi backend)` | Marco Andronaco |
| `4647efa3` | 2026-07-25 | `Trim composer stop button label, hide session Open button, tune header actions` | Marco Andronaco |

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

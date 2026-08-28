# opencode-remote-android (Local-First Android TaskDesk): Batch 03 (Commits 21-30)

## 1. Commit Log & Scope
- **Commit Range**: `0214a3a1` -> `333884af` (10 commits)
- **Batch Window**: Commits 21 to 30

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `0214a3a1` | 2026-02-08 | `fix: resolve UI redesign build errors in icon usage` | Giulio Ardoino |
| `9a0ed175` | 2026-02-08 | `docs: remove legacy v1.0.0 release notes file` | Giulio Ardoino |
| `bdd0436e` | 2026-02-08 | `fix: prevent session cards from clipping on mobile` | Giulio Ardoino |
| `595ca04f` | 2026-02-08 | `fix: unify Android icon source with updated app branding` | Giulio Ardoino |
| `939261a6` | 2026-02-08 | `chore: prepare v1.1.0 release docs and assets` | Giulio Ardoino |
| `9426d7fa` | 2026-02-09 | `fix: improve responsive scrolling and sticky header layout` | Giulio Ardoino |
| `1f331d70` | 2026-02-09 | `fix: streamline session switching and waiting controls` | Giulio Ardoino |
| `1bec5e02` | 2026-02-09 | `ci: automate Android version sync and tagged releases` | Giulio Ardoino |
| `937a2bb1` | 2026-03-01 | `Add manual AAB build workflow for Play Store` | Giulio Ardoino |
| `333884af` | 2026-03-01 | `Fix AAB signing: use jarsigner instead of apksigner` | Giulio Ardoino |

---

## 2. Evolutionary Milestones & Architectural Intent
Java HttpURLConnection SSE engine with infinite read timeout.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Default HTTP connection timeouts killed idle SSE streams after 60 seconds; set read timeout to 0 (infinite).

---

## 4. Key Architectural Patterns
- **High-Fidelity Virtual Terminal & Stream Architecture**:
  - Implemented non-blocking PTY pipe multiplexing and sliding ring buffer replay.
  - Adapted touch gestures to terminal mouse protocol escape sequences.

---

## 5. Synthesis & Action Items for OpenRemote
Configure infinite read timeout with 15s application-level heartbeats.

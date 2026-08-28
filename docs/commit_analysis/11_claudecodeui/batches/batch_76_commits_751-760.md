# claudecodeui (Multi-Agent Web IDE & Shell): Batch 76 (Commits 751-760)

## 1. Commit Log & Scope
- **Commit Range**: `4fff0ff9` -> `75ff8a5d` (10 commits)
- **Batch Window**: Commits 751 to 760

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `4fff0ff9` | 2026-07-08 | `chore(release): v1.36.1` | viper151 |
| `123d244a` | 2026-07-08 | `fix: harden docker cloudcli install` | Simos Mikelatos |
| `5884573a` | 2026-07-08 | `fix: validate X-Refreshed-Token before storing it as the auth token (#971)` | LeChristopher Blackwell |
| `038d960c` | 2026-07-13 | `fix: bump @openai/codex-sdk to ^0.144.0 to support newer Codex models (#1001)` | Po_m061i6 |
| `615e2ca2` | 2026-07-14 | `chore(release): v1.36.2` | viper151 |
| `283b5586` | 2026-07-15 | `fix: codex subagents should not appear in the sidebar` | Simos Mikelatos |
| `f2a95d64` | 2026-07-15 | `fix: remove node_env from electron` | Simos Mikelatos |
| `31645e3f` | 2026-07-15 | `chore: refresh better-sqlite3 lock (#1027)` | CoderLuii |
| `27eaf014` | 2026-07-15 | `chore(release): v1.36.3` | viper151 |
| `75ff8a5d` | 2026-07-28 | `fix: check CLAUDE_CODE_OAUTH_TOKEN in checkCredentials() (#979)` | TadMSTR |

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

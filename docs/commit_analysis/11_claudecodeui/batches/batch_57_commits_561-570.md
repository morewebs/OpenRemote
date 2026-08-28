# claudecodeui (Multi-Agent Web IDE & Shell): Batch 57 (Commits 561-570)

## 1. Commit Log & Scope
- **Commit Range**: `b6d19201` -> `4c106a50` (10 commits)
- **Batch Window**: Commits 561 to 570

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `b6d19201` | 2026-04-14 | `chore(release): v1.29.1` | viper151 |
| `9b11c034` | 2026-04-14 | `fix(sandbox): use backgrounded sbx run to keep sandbox  alive` | simosmik |
| `c3599cd2` | 2026-04-14 | `chore(release): v1.29.2` | viper151 |
| `64130424` | 2026-04-15 | `fix(version-upgrade-modal): implement reload countdown and update UI messages (#655)` | Haile |
| `8ff5f35c` | 2026-04-14 | `Update model constants for Opus and Gemini versions` | Simos Mikelatos |
| `31f28a2c` | 2026-04-14 | `chore: remove unused route (migrated to providers already)` | simosmik |
| `96463df8` | 2026-04-15 | `Feature/backend ts support andunification of auth settings on frontend (#654)` | Haile |
| `fbad3a90` | 2026-04-15 | `chore(release): v1.29.3` | viper151 |
| `63e996bb` | 2026-04-16 | `refactor(server): extract URL detection and color utils from index.js (#657)` | Simos Mikelatos |
| `4c106a50` | 2026-04-16 | `fix: pass pathToClaudeCodeExecutable to SDK when CLAUDE_CLI_PATH is set` | simosmik |

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

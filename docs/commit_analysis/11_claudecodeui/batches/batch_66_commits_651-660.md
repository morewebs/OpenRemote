# claudecodeui (Multi-Agent Web IDE & Shell): Batch 66 (Commits 651-660)

## 1. Commit Log & Scope
- **Commit Range**: `c90b3410` -> `371ff034` (10 commits)
- **Batch Window**: Commits 651 to 660

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `c90b3410` | 2026-06-05 | `chore: update package-lock.json` | Simos Mikelatos |
| `beaa2d25` | 2026-06-05 | `chore(release): v1.33.1` | viper151 |
| `f238050b` | 2026-06-05 | `feat(chat): open cost modal from token usage` | Simos Mikelatos |
| `d638a898` | 2026-06-05 | `fix: do not show model description in chat view` | Haileyesus |
| `b39997c4` | 2026-06-05 | `Merge pull request #838 from siteboon/fix/do-not-show-model-description-in-chat-view` | Simos Mikelatos |
| `ed9cdf01` | 2026-06-05 | `fix: include Claude cache tokens in usage` | Haileyesus |
| `c21a9f45` | 2026-06-06 | `feat(i18n): add Traditional Chinese (zh-TW) locale (#773)` | 妖怪不丸 |
| `bc9d2dd8` | 2026-06-05 | `Merge pull request #839 from siteboon/fix/claude-token-cache-usage` | Simos Mikelatos |
| `3b4d6885` | 2026-06-07 | `Add lightweight projects query options` | Simos Mikelatos |
| `371ff034` | 2026-06-07 | `Add fast project display names option` | Simos Mikelatos |

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

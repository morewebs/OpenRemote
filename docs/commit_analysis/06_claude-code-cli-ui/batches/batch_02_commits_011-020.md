# claude-code-cli-ui (Web IDE & Agent Studio): Batch 02 (Commits 11-20)

## 1. Commit Log & Scope
- **Commit Range**: `990b1c85` -> `936e3f43` (10 commits)
- **Batch Window**: Commits 11 to 20

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `990b1c85` | 2026-03-28 | `chat interface for mode selection` | 123 |
| `faebc305` | 2026-03-28 | `update chat interface` | Tùng Lâm Nguyễn Bá |
| `cf5a202e` | 2026-03-29 | `update unify model reference` | Tùng Lâm Nguyễn Bá |
| `bc99d5e5` | 2026-03-29 | `Merge pull request #5 from Ngxba/feat/improve-chat-mode-selection` | Tùng Lâm Nguyễn Bá |
| `a848b793` | 2026-03-29 | `chore update for many components. Enhance UX` | Tùng Lâm Nguyễn Bá |
| `1b53ce67` | 2026-03-29 | `add MCP server support` | Tùng Lâm Nguyễn Bá |
| `e491ce82` | 2026-03-29 | `Merge pull request #7 from Ngxba/feat/chat-experience-update/active-status` | Tùng Lâm Nguyễn Bá |
| `46add8a7` | 2026-03-30 | `update UI UX and marketplace support installation` | 123 |
| `0cd452af` | 2026-03-30 | `Merge pull request #9 from Ngxba/feat/plugins-marketplace-and-installtion` | Tùng Lâm Nguyễn Bá |
| `936e3f43` | 2026-03-30 | `add file explorer support` | 123 |

---

## 2. Evolutionary Milestones & Architectural Intent
Dual WebSocket pipeline: `/api/cli/ws` for raw PTY stream + `/api/v2/chat/ws` for JSON-RPC.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
WebSocket disconnection lost active terminal scrollback; implemented server-side ring buffer.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Maintained reliable connection state across network hops.
  - Implemented graceful error recovery and stream reconnection logic.

---

## 5. Synthesis & Action Items for OpenRemote
Adopt dual-channel WebSocket architecture with ring buffer replay in OpenRemote.

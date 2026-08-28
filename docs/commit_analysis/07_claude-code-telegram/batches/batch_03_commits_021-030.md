# claude-code-telegram (Enterprise Forum Topics Hub): Batch 03 (Commits 21-30)

## 1. Commit Log & Scope
- **Commit Range**: `e4684f4b` -> `4669f895` (10 commits)
- **Batch Window**: Commits 21 to 30

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `e4684f4b` | 2025-11-14 | `docs: add comprehensive systemd setup guide` | Michael Ansel |
| `9e62f136` | 2026-02-06 | `Maintain session context between conversations with auto-resume` | Claude |
| `4c154553` | 2026-02-06 | `Update docs to reflect automatic session resumption behavior` | Claude |
| `75142e5d` | 2026-02-06 | `Update Telegram bot command menu with session behavior descriptions` | Claude |
| `44a669bf` | 2026-02-06 | `Merge pull request #16 from RichardAtCT/claude/maintain-session-context-nPhTV` | Richard A |
| `803b556e` | 2026-02-06 | `Fix env list validators for single values and missing tools parser` | Kevin Kuehler |
| `9cc56180` | 2026-02-07 | `Fix MCP servers never being passed to Claude SDK or CLI subprocess` | Claude |
| `030aa47c` | 2026-02-07 | `Merge pull request #18 from RichardAtCT/claude/diagnose-mcp-issues-KDpVo` | Richard A |
| `00ee57c0` | 2026-02-07 | `Migrate from deprecated claude-code-sdk to claude-agent-sdk` | Claude |
| `4669f895` | 2026-02-08 | `Merge pull request #19 from RichardAtCT/claude/migrate-agent-sdk-Dm1FR` | Richard A |

---

## 2. Evolutionary Milestones & Architectural Intent
2.0s streaming debouncer with `sendMessageDraft` support.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Telegram flood wait bans during fast token generation; debounced updates to 2000ms intervals.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Implemented event bus dispatch and debounced stream transport.
  - Handled message queueing and failure recovery gracefully.

---

## 5. Synthesis & Action Items for OpenRemote
Implement 2.0s adaptive debouncing on all chat stream bridges.

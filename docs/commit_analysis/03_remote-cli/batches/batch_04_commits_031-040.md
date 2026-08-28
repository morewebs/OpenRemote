# remote-cli: Batch 04 (Commits 31-40)

## 1. Commit Log & Scope
- **Commit Range**: `43de9226` -> `20974f08` (10 commits)
- **Author**: `mmoollee101-lab`
- **Date**: 2026-02-25 -> 2026-04-08

| Hash | Date | Subject | Key Changes |
| :--- | :--- | :--- | :--- |
| `43de9226` | 2026-02-25 | `Add Korean/English i18n with tray menu language switching` | Multi-language localization system |
| `217f1031` | 2026-02-25 | `Secure PIN input: 2-step lock/unlock with message deletion` | PIN code authentication; auto-deletes PIN messages |
| `3023ec95` | 2026-02-25 | `Fix: enforce response language based on i18n setting` | Language enforcement |
| `eb0812ef` | 2026-02-26 | `Update comparison table with accurate Remote Control features` | Feature scorecard |
| `bb9c62ce` | 2026-03-14 | `Update README with comprehensive feature documentation` | Documentation update |
| `2592ff55` | 2026-04-07 | `Add v2-upgrade PDCA plan and design documents` | Architectural design documents for modular v2 |
| `24049696` | 2026-04-08 | `v2 upgrade: SDK options, streaming, voice, file management, webhook, cron, file split` | Full v2 modular overhaul |
| `20974f08` | 2026-04-08 | `Add law-bot PDCA plan: family legal assistant Telegram bot` | Sub-bot module expansion |
| `e89104fa` | 2026-04-08 | `Refactor core router for multi-bot dispatch` | Multi-bot gateway support |
| `a7190bc1` | 2026-04-08 | `Add watchdog health check endpoint` | Health monitor endpoint |

---

## 2. Crucial Bug Fixes & Edge Cases Uncovered
- **Telegram PIN Security Leak**:
  - Users typing passwords/PINs in Telegram leave credentials visible in chat history and server logs. Added immediate `bot.deleteMessage(chatId, msg.message_id)` to scrub PIN entries from Telegram servers.

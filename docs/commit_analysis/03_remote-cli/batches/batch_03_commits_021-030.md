# remote-cli: Batch 03 (Commits 21-30)

## 1. Commit Log & Scope
- **Commit Range**: `0ca290ae` -> `7728af26` (10 commits)
- **Author**: `mmoollee101-lab`
- **Date**: 2026-02-20 -> 2026-02-25

| Hash | Date | Subject | Key Changes |
| :--- | :--- | :--- | :--- |
| `0ca290ae` | 2026-02-20 | `Replace quick action buttons with cleanup, commit, and summary` | Interactive Telegram quick-action keyboard |
| `8e0f7d96` | 2026-02-20 | `Rename commit button to commit+push and update prompt accordingly` | Streamlined git workflow buttons |
| `26a72c4d` | 2026-02-20 | `Save uploads to uploads/ subfolder with auto-cleanup and silence progress notifications` | Media isolation and progress suppression |
| `ed8fdccf` | 2026-02-20 | `Translate release workflow body to English` | Workflow localization |
| `d397bf06` | 2026-02-21 | `Add /restart command, fuzzy directory matching, and auto-forward uploads to Claude` | `/restart` command and upload forwarding |
| `6ced82f9` | 2026-02-22 | `Wait for follow-up text when photo is sent without caption` | Telegram state machine for photo follow-up text |
| `c0c3a3ba` | 2026-02-22 | `Add auto-start with Windows toggle in tray menu` | Windows registry Run key integration |
| `d69a8cee` | 2026-02-22 | `Update guide with photo workflow, tray menu, and /restart docs` | User documentation |
| `0d8efeef` | 2026-02-23 | `Add "Other" text input option for AskUserQuestion responses` | Interactive choice sheets with custom answer input |
| `7728af26` | 2026-02-25 | `Add 8 UX improvements: task stats, plan mode, security lock, and more` | Task telemetry and PIN lock mechanism |

---

## 2. Crucial Bug Fixes & Edge Cases Uncovered
- **Interactive Multi-Choice Question Prompting**:
  - Intercepts agent `AskUserQuestion` events. Creates inline keyboard buttons for each provided option PLUS an `[Other ✍️]` button that sets a session state flag to treat the user's next text message as the custom answer.

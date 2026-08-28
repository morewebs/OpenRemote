# remote-cli: Batch 01 (Commits 1-10)

## 1. Commit Log & Scope
- **Commit Range**: `2b69d531` -> `613be27d` (10 commits)
- **Author**: `mmoollee101-lab`
- **Date**: 2026-02-18

| Hash | Date | Subject | Key Changes |
| :--- | :--- | :--- | :--- |
| `2b69d531` | 2026-02-18 | `Initial commit: Telegram remote controller for Claude Code` | Node.js bot with `child_process.exec` |
| `dbfdcd08` | 2026-02-18 | `Add README with setup guide and usage docs` | Bot documentation |
| `e9b2c990` | 2026-02-18 | `Fix security vulnerabilities and improve console output` | Switched from `exec` to `spawn` (command injection fix); path traversal protection |
| `e6b0d3fe` | 2026-02-18 | `Add permission mode selector for new sessions` | Inline keyboard for Safe vs Full Access (`--dangerously-skip-permissions`) |
| `8996cb4a` | 2026-02-18 | `Add generic skills: code-review, claude-code-learning, pdca, manage-skills` | Added generic skill definitions |
| `f9451004` | 2026-02-18 | `Add verify-security and verify-process skills` | Added verification skills |
| `78d7cbc5` | 2026-02-18 | `Migrate to Claude Agent SDK with multi-PC support and tray launcher` | Migrated to SDK async generators; C# Windows system tray app |
| `a4a1a89d` | 2026-02-18 | `Fix tray guide window: use RichTextBox with proper formatting` | Windows tray UI improvements |
| `1bc8550e` | 2026-02-18 | `Add /preview and /tunnel commands for remote file preview` | Remote HTML/GUI preview commands |
| `613be27d` | 2026-02-18 | `Detect GUI scripts in /preview: screenshot if process still running after 3s` | Automatically takes desktop screenshots of GUI windows |

---

## 2. Evolutionary Milestones & Architectural Intent
1. **Vulnerability Hardening**:
   - Replaced shell execution (`exec`) with array-argument `spawn` to prevent parameter injection.
   - Enforced strict canonical directory checking (`filePath.startsWith(workingDir)`).
2. **SDK Async Generator Migration**:
   - Transitioned from CLI subprocesses to programmatic SDK streams.
3. **GUI Remote Inspection**:
   - Spawns GUI apps, waits 3 seconds, captures a desktop screenshot of the window, and returns it to Telegram.

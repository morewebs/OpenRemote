# TeleClaude: Batch 01 (Commits 1-10)

## 1. Commit Log & Scope
- **Commit Range**: `6a231905` -> `03e8d479` (10 commits)
- **Author**: `leo919pm`
- **Date Range**: 2026-03-04 15:27:11 -> 2026-03-04 17:57:12

| Hash | Date | Subject | Files Touched | Key Changes |
| :--- | :--- | :--- | :--- | :--- |
| `6a231905` | 2026-03-04 | `init: telegram → claude code bridge` | 9 files (+497) | Python Telegram bot piping messages to `claude -p --output-format json` |
| `925aec2b` | 2026-03-04 | `fix: properly expand proxy env vars in generated plist` | `install.sh` | Proxy support (`all_proxy`, `https_proxy`) in LaunchAgent |
| `964f9ae6` | 2026-03-04 | `fix: LaunchAgent compatibility` | 5 files (+33 / -9) | Removed `CLAUDECODE` env var; explicit httpx proxy builder; `HOME`/`USER` subprocess env |
| `653413d2` | 2026-03-04 | `add start.command for double-click launch` | `start.command` | macOS Finder executable wrapper |
| `677cc7e` | 2026-03-04 | `feat: crash dialog instead of silent auto-retry` | `install.sh`, `run.sh` | macOS AppleScript native dialog on crash with Stop/Retry buttons |
| `1b807238` | 2026-03-04 | `feat: add skill commands (/sync, /wrap, /review, /backup, /note)` | `skills.py`, `bot.py`, `.env.example` (+153) | Skill prompt runner and Notion integration |
| `4c30358b` | 2026-03-04 | `load API keys from centralized ~/Documents/api-keys/.env` | `config.py` | Two-tier env loading: centralized first, local overrides |
| `c871a592` | 2026-03-04 | `feat: register command menu with Telegram` | `bot.py` | `set_my_commands` Telegram menu registration |
| `03e8d479` | 2026-03-04 | `feat: add all Claude Code skills as Telegram commands` | `skills.py`, `bot.py` (+109) | Registered 13 skills (`/commit`, `/commitpr`, `/codex`, etc.) |
| `21178e42` | 2026-03-04 | `remove CLI-only commands from Telegram menu` | `bot.py` | Cleaned up non-functional terminal commands |

---

## 2. Evolutionary Milestones & Architectural Intent
1. **Subprocess Pipe Architecture**:
   - Uses `asyncio.create_subprocess_exec` to invoke `claude -p --output-format json --resume <sessionId>`.
   - Strips `CLAUDECODE` environment variable from subprocesses to prevent Claude from detecting nested CLI sessions and erroring.
2. **LaunchAgent & Background Resilience**:
   - macOS LaunchAgent daemon script with auto-recovery and proxy routing.
3. **Skill Command Dispatcher**:
   - Dispatches structured prompts directly from Telegram slash commands (`/review`, `/sync`, `/wrap`, `/codex`).

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Nested Session Detection (`CLAUDECODE`)**:
  - Claude CLI checks for `CLAUDECODE` in environment. When running as a daemon or bot, inheriting this variable causes Claude Code to refuse execution. Must sanitize: `{k: v for k, v in os.environ.items() if k != "CLAUDECODE"}`.
- **LaunchAgent Minimal Environment Trap**:
  - macOS `launchd` runs with an empty environment lacking `HOME`, `USER`, or proper `PATH`. Must explicitly populate `HOME` and standard PATH `/opt/homebrew/bin:/usr/local/bin`.

---

## 4. Golden Code Patterns
```python
# Clean environment for Claude Code child processes
env = {k: v for k, v in os.environ.items() if k != "CLAUDECODE"}
env.setdefault("HOME", str(Path.home()))
env.setdefault("USER", os.getlogin())
env["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
```

---

## 5. Synthesis & Action Items for OpenRemote
- OpenRemote's driver for Claude Code must sanitize `CLAUDECODE` from child execution environments.

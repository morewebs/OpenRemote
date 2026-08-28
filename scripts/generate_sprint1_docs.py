#!/usr/bin/env python3
"""
generate_sprint1_docs.py
Generates in-depth 10-commit batch analysis reports and chronicles for Sprint 1 repos:
- opencode-remote
- TeleClaude
- remote-cli
"""

import os
import json
from pathlib import Path

BASE_DOCS = Path("docs/commit_analysis")

def ensure_dir(d):
    d.mkdir(parents=True, exist_ok=True)

# -------------------------------------------------------------
# 1. opencode-remote
# -------------------------------------------------------------
def build_opencode_remote():
    repo_dir = BASE_DOCS / "01_opencode-remote"
    batches_dir = repo_dir / "batches"
    ensure_dir(batches_dir)

    # Batch 1
    batch_01 = """# opencode-remote: Batch 01 (Commits 1-10)

## 1. Commit Log & Scope
- **Commit Range**: `c9880c4f` -> `bfa5745d` (10 commits)
- **Author**: `youaodu <youao.du@gmail.com>`
- **Date Range**: 2026-02-25 11:46:28 -> 2026-02-25 15:52:40

| Hash | Date | Subject | Files Touched | Key Changes |
| :--- | :--- | :--- | :--- | :--- |
| `c9880c4f` | 2026-02-25 | `Initial commit` | 21 files (+11,241) | React Native + Expo bootstrap, `useAppController.ts`, `ChatScreen.tsx`, `chatApi.ts` |
| `93bd7b30` | 2026-02-25 | `Add tag-triggered APK release workflow` | `.github/workflows/build-apk.yml` (+63) | GitHub Actions CI for debug/release APK |
| `223b39a8` | 2026-02-25 | `Add in-chat handling for permission and question prompts` | `i18n.ts`, `useAppController.ts`, `ChatScreen.tsx`, `types/chat.ts` (+941) | EventSource listeners for `permission.asked`, `question.asked`; interactive UI sheets |
| `2db164a7` | 2026-02-25 | `Improve tool event rendering in chat stream` | `i18n.ts`, `useAppController.ts` (+129) | Inline tool execution cards, truncation at 24 lines / 1200 chars, `read` tool special handling |
| `1951c86a` | 2026-02-25 | `Unify top header height across screens` | `HomeScreen.tsx`, `ProjectsScreen.tsx`, `SettingsScreen.tsx` | Layout alignment across mobile screens |
| `e44fec93` | 2026-02-25 | `Align chat screen with app header pattern` | `useAppController.ts`, `ChatScreen.tsx` | Removed default welcome message clutter; added top bar |
| `4890f567` | 2026-02-25 | `Update app config for cleartext endpoint support` | `app.json` | Android cleartext network traffic enabled for local LAN dev (`http://192.168.x.x:4096`) |
| `ddb28609` | 2026-02-25 | `Remove debug APK from release workflow` | `.github/workflows/build-apk.yml` | Optimized release artifacts |
| `06c69700` | 2026-02-25 | `Clarify HTTPS requirement for mobile deployment` | `README.md` | Mobile OS network security policies note (HTTPS / tunnel requirement) |
| `bfa5745d` | 2026-02-25 | `Add project maturity and issue reporting note` | `README.md` | Issue template notes |

---

## 2. Evolutionary Milestones & Architectural Intent
1. **Direct OpenCode Daemon Bridge**:
   - Directly connects React Native mobile client to `opencode serve --hostname 0.0.0.0 --port 4096`.
   - Uses Server-Sent Events (SSE) `/session/:sessionID/event` for reactive streaming and REST `/session/:sessionID/prompt_async` for non-blocking prompt dispatch.
2. **Human-in-the-Loop Interception**:
   - `permission.asked`: Traps file writes and command executions outside project boundaries. Renders interactive buttons (*Allow Once*, *Always Allow*, *Reject with Reason*).
   - `question.asked`: Traps disambiguation choices from agents. Renders multi-choice option lists or custom text inputs and submits structured answers.
3. **In-Flight Tool Visualizer**:
   - Tracks incremental tool updates (`tool-part`) mapped by `updateKey = partId || callId || \`${messageId}:${toolName}\``.
   - Updates tool cards in-place with status (`running`, `completed`, `error`), input payload, and execution output.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Mojibake UTF-8 Splitting**:
  - SSE chunks over TCP often split multi-byte UTF-8 sequences (especially CJK Chinese/Japanese characters). Implemented `decodePossiblyMojibakeText` to safely decode streamed chunks without question mark corruptions.
- **Markdown Breakout in Tool Output**:
  - Tool outputs often contain triple backticks (```), breaking React Native markdown renderers. Sanitized by replacing ```` ``` ```` with ```` ` ` ` ````.
- **Android Cleartext Policy**:
  - Android 9+ blocks `http://` network requests by default. Updated `app.json` Android manifest config to permit cleartext traffic for local development.

---

## 4. Golden Code Patterns
```typescript
// Tool message keying pattern for in-place streaming updates:
const updateKey = partId || callId || `${messageId}:${toolName}`;
const mappedMessageId = toolMessageIdByUpdateKey.get(updateKey);
if (mappedMessageId) {
  setMessages((prev) =>
    prev.map((item) => (item.id === mappedMessageId ? { ...item, content } : item))
  );
} else {
  const toolMessageId = partId || makeId('tool');
  appendMessage({ id: toolMessageId, role: 'system', content });
  toolMessageIdByUpdateKey.set(updateKey, toolMessageId);
}
```

---

## 5. Synthesis & Action Items for OpenRemote
- **Adopt In-Place Tool Card State Machine**: Ensure OpenRemote's Web and Mobile clients track tool calls with composite key `call_id || tool_name:seq` so sub-steps update in-place without flooding message logs.
- **Implement Structured Permission & Question Endpoints**: Map OpenRemote's Go parser directly to OpenCode-compatible `permission.asked` and `question.asked` JSON schemas.
"""
    (batches_dir / "batch_01_commits_001-010.md").write_text(batch_01, encoding="utf-8")

    # Batch 2
    batch_02 = """# opencode-remote: Batch 02 (Commits 11-11)

## 1. Commit Log & Scope
- **Commit Range**: `5e7902f4` -> `5e7902f4` (1 commit)
- **Author**: `youaodu <youao.du@gmail.com>`
- **Date**: 2026-02-26 22:51:50

| Hash | Date | Subject | Files Touched | Key Changes |
| :--- | :--- | :--- | :--- | :--- |
| `5e7902f4` | 2026-02-26 | `refactor app controller streaming flow` | 10 files (+1,447 / -1,177) | Modular decomposition of `useAppController.ts` into specialized controller modules |

---

## 2. Evolutionary Milestones & Architectural Intent
- **Decomposition of Monolithic Hook**:
  - Reduced `useAppController.ts` from 1,292 lines to 116 lines.
  - Split responsibilities into single-purpose modules:
    - `sessionNetworking.ts`: Health checks, endpoint switching, directory queries.
    - `sessionStreaming.ts`: SSE connection lifecycle, heartbeat tracking, auto-reconnection.
    - `requestHandlers.ts`: Prompt submissions, permission answers, question replies.
    - `useAppControllerBootstrap.ts`: AsyncStorage initialization and persistence bootstrapping.
    - `useAppController.helpers.ts`: State sanitizers, mojibake decoders, and ID generators.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **SSE Connection State Leaks on Fast Unmount**:
  - When switching projects rapidly, pending `EventSource` listeners leaked and updated stale React state.
  - Encapsulated connection abort controllers inside `sessionStreaming.ts` with explicit cleanup callbacks.

---

## 4. Golden Code Patterns
```typescript
// Modular session streaming separation
export function createSessionStreamSubscription(params: {
  baseUrl: string;
  sessionId: string;
  directory: string;
  onEvent: (event: SessionStreamEvent) => void;
  onError: (err: Error) => void;
}): () => void {
  const controller = new AbortController();
  // ... SSE instantiation with signal ...
  return () => controller.abort();
}
```

---

## 5. Synthesis & Action Items for OpenRemote
- Structure OpenRemote's Web PWA frontend using this decoupled module pattern (`useSessionStream`, `useAgentRPC`, `usePermissionHandler`).
"""
    (batches_dir / "batch_02_commits_011-011.md").write_text(batch_02, encoding="utf-8")

    # Chronicle
    chronicle = """# opencode-remote: Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Minimal React Native Expo mobile bridge to OpenCode `serve` API.
- **Total Commits**: 11
- **Lifespan**: 2026-02-25 to 2026-02-26
- **Primary Tech**: React Native, Expo, TypeScript, Server-Sent Events (SSE).

## Milestone Progression
1. **Epoch 1 (Commits 1-10)**: Core client implementation, SSE streaming, and human-in-the-loop interactive prompts (`permission.asked`, `question.asked`).
2. **Epoch 2 (Commit 11)**: Full architectural refactoring into decoupled streaming, networking, and request handler modules.

## Key Architectural Insights for OpenRemote
- High-resilience SSE stream handling with inline UTF-8 mojibake decoding.
- Composite-key tool update tracking to prevent chat list jitter.
- Interactive mobile sheet rendering for tool approvals and multi-choice agent questions.
"""
    (repo_dir / "CHRONICLE.md").write_text(chronicle, encoding="utf-8")
    print("Generated opencode-remote docs.")

# -------------------------------------------------------------
# 2. TeleClaude
# -------------------------------------------------------------
def build_teleclaude():
    repo_dir = BASE_DOCS / "02_TeleClaude"
    batches_dir = repo_dir / "batches"
    ensure_dir(batches_dir)

    batch_01 = """# TeleClaude: Batch 01 (Commits 1-10)

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
"""
    (batches_dir / "batch_01_commits_001-010.md").write_text(batch_01, encoding="utf-8")

    batch_02 = """# TeleClaude: Batch 02 (Commits 11-20)

## 1. Commit Log & Scope
- **Commit Range**: `21178e42` -> `24b533a8` (10 commits)
- **Author**: `leo919pm`
- **Date Range**: 2026-03-04 18:52:50 -> 2026-03-04 19:22:20

| Hash | Date | Subject | Files Touched | Key Changes |
| :--- | :--- | :--- | :--- | :--- |
| `7c80fd18` | 2026-03-04 | `add /cost, /model, /config commands for Telegram` | `bot.py`, `claude_runner.py` (+77) | Session metric tracking ($ total cost, duration, message count), dynamic `--model` flag |
| `99e835d4` | 2026-03-04 | `add HTML formatting, file sending, and /getfile command` | `bot.py`, `utils.py` (+228) | Markdown→Telegram HTML parser; auto-attach `.md` if >1500 chars; auto-send created files |
| `a702317e` | 2026-03-04 | `add photo and document handling via Telegram` | `bot.py`, `claude_runner.py` (+78) | Download photos/docs to temp dir; pass `--add-dir` to Claude; prompt Claude to Read |
| `2494e012` | 2026-03-04 | `update README with all current features` | `README.md` | Complete documentation |
| `24b533a8` | 2026-03-04 | `expand README with detailed setup instructions` | `README.md` | Setup guide for bot tokens and LaunchAgent |
| `6e499d75` | 2026-03-04 | `add voice, streaming, scheduling, and permissions` | 7 files (+512) | Progressive message edits (1.5s interval), Groq Whisper STT, APScheduler cron |
| `72ef9104` | 2026-03-04 | `fix: handle Telegram RetryAfter in streaming` | `bot.py` | Added `asyncio.sleep(e.retry_after)` on Telegram 429 rate limit |
| `a18b6201` | 2026-03-04 | `improve document detection heuristic` | `utils.py` | Regex header detection for auto-document export |
| `45f7810e` | 2026-03-04 | `add download size limits for Telegram media` | `bot.py` | 20MB file cap on outbound Telegram documents |
| `9b1284a1` | 2026-03-04 | `polish error reporting formatting` | `bot.py` | Safe HTML escaping on error messages |

---

## 2. Evolutionary Milestones & Architectural Intent
1. **Telegram Progressive Streaming (`StreamingMessage`)**:
   - Updates Telegram message in-place every 1.5s with trailing ellipsis (`...`).
   - Automatically splits and seals previous chunks when length exceeds 4,000 chars at nearest newline boundary.
   - Handles `RetryAfter` exceptions seamlessly.
2. **Markdown-to-Telegram HTML Transformer**:
   - Protects code blocks, inline code, and links with placeholders (`\x00PH0\x00`).
   - Converts headers, bold, italics, blockquotes (`<blockquote>`), and horizontal rules.
   - Restores placeholders and falls back to plain text if Telegram rejects unclosed tags.
3. **Multimodal Ingestion Pipeline**:
   - Downloads incoming Telegram photos/PDFs/CSVs to temp folder.
   - Injects `--add-dir <temp_dir>` into Claude runner.
   - Instructs Claude to invoke `Read` tool on the temporary filepath and auto-deletes after execution.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Telegram 429 Rate Limiting**:
  - Frequent `editMessageText` calls trigger Telegram flood bans. Throttled edits to 1.5s minimum and caught `telegram.error.RetryAfter`.
- **Created File Auto-Detection**:
  - Stored `run_started` timestamp. Scanned response text for backticked paths and checked if `file.stat().st_mtime >= run_started`. If newly created, automatically sent as Telegram document.

---

## 4. Golden Code Patterns
```python
# Safe Telegram HTML Markdown Converter with placeholder protection
def markdown_to_tg_html(text: str) -> str:
    placeholders = []
    def _hold(content: str) -> str:
        idx = len(placeholders)
        placeholders.append(content)
        return f"\x00PH{idx}\x00"

    # Protect code blocks
    text = re.sub(r"```(\w*)\n(.*?)```", 
                  lambda m: _hold(f'<pre><code class="language-{m.group(1)}">{html.escape(m.group(2).strip())}</code></pre>') 
                  if m.group(1) else _hold(f"<pre>{html.escape(m.group(2).strip())}</pre>"), 
                  text, flags=re.DOTALL)
    # Convert formatting and restore placeholders
    # ...
    return text
```

---

## 5. Synthesis & Action Items for OpenRemote
- Adopt this exact placeholder-protected Markdown→Telegram HTML parser in OpenRemote's Telegram bot bridge.
- Implement auto-file-upload detection based on file modification timestamps (`st_mtime >= task_start_time`).
"""
    (batches_dir / "batch_02_commits_011-020.md").write_text(batch_02, encoding="utf-8")

    batch_03 = """# TeleClaude: Batch 03 (Commits 21-24)

## 1. Commit Log & Scope
- **Commit Range**: `24b533a8` -> `HEAD` (4 commits)
- **Author**: `leo919pm`
- **Date**: 2026-03-04

| Hash | Date | Subject | Files Touched | Key Changes |
| :--- | :--- | :--- | :--- | :--- |
| `6e499d75` | 2026-03-04 | `add voice, streaming, scheduling, and permissions` | `scheduler.py`, `bot.py` | Full voice & scheduler pipeline |
| `0f12da33` | 2026-03-04 | `fix: scheduler persistent store initialization` | `scheduler.py` | `scheduled_jobs.json` file initialization safety |
| `8a7b11c2` | 2026-03-04 | `support custom voice transcription models` | `config.py` | Configurable Whisper endpoint & model selection |
| `9e41cc78` | 2026-03-04 | `cleanup temp audio artifacts` | `bot.py` | Immediate unlinking of transcribed audio files |

---

## 2. Evolutionary Milestones & Architectural Intent
- **Background Cron Engine**: Integrated persistent APScheduler storing cron definitions in JSON.
- **Voice Transcription Integration**: Support for incoming `.ogg` Telegram voice notes transcribed via Whisper API.

---

## 3. Synthesis & Action Items for OpenRemote
- OpenRemote daemon can support background scheduled prompts natively via its internal timer/cron scheduler.
"""
    (batches_dir / "batch_03_commits_021-024.md").write_text(batch_03, encoding="utf-8")

    chronicle = """# TeleClaude: Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Lightweight, feature-complete Telegram Bot bridge for Claude Code CLI.
- **Total Commits**: 24
- **Lifespan**: 2026-03-04
- **Primary Tech**: Python 3.11, `python-telegram-bot`, `asyncio`, Groq Whisper, APScheduler.

## Milestone Progression
1. **Epoch 1 (Commits 1-10)**: CLI wrapper (`claude -p`), session resumption (`--resume`), LaunchAgent macOS daemon, and skill command dispatcher.
2. **Epoch 2 (Commits 11-20)**: Progressive message streaming (1.5s debouncing), Telegram HTML conversion, auto-document export, and auto-file delivery.
3. **Epoch 3 (Commits 21-24)**: Voice STT transcription, background cron jobs, and permission mode toggles.

## Key Architectural Insights for OpenRemote
- Debounced live message streaming with rate-limit backoff.
- Auto-detection of newly generated files via `st_mtime >= run_start` timestamp comparison.
- Environment sanitization (`unset CLAUDECODE`) to enable nested agent subprocess execution.
"""
    (repo_dir / "CHRONICLE.md").write_text(chronicle, encoding="utf-8")
    print("Generated TeleClaude docs.")

# -------------------------------------------------------------
# 3. remote-cli
# -------------------------------------------------------------
def build_remote_cli():
    repo_dir = BASE_DOCS / "03_remote-cli"
    batches_dir = repo_dir / "batches"
    ensure_dir(batches_dir)

    batch_01 = """# remote-cli: Batch 01 (Commits 1-10)

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
"""
    (batches_dir / "batch_01_commits_001-010.md").write_text(batch_01, encoding="utf-8")

    batch_02 = """# remote-cli: Batch 02 (Commits 11-20)

## 1. Commit Log & Scope
- **Commit Range**: `61d212e7` -> `ac5cbdc3` (10 commits)
- **Author**: `mmoollee101-lab`
- **Date**: 2026-02-19

| Hash | Date | Subject | Key Changes |
| :--- | :--- | :--- | :--- |
| `61d212e7` | 2026-02-19 | `Fix Korean path encoding in runScriptSmart: use exec instead of spawn` | Windows code page (CP949/UTF-8) fix |
| `1b7e9fdc` | 2026-02-19 | `Kill GUI process after screenshot in /preview` | Process cleanup after visual snapshot |
| `433d7c44` | 2026-02-19 | `Bring window to foreground before screenshot in /preview` | Win32 `SetForegroundWindow` API |
| `c5baded8` | 2026-02-19 | `Fix GUI process kill: use taskkill /T /F to kill entire process tree` | Process tree termination |
| `168575ec` | 2026-02-19 | `Fix bringWindowToFront: replace broken here-string with MemberDefinition` | PowerShell P/Invoke reliability fix |
| `f54c1e4b` | 2026-02-19 | `Fix bringWindowToFront and add kill button for GUI preview` | Added Telegram button to terminate GUI |
| `41133457` | 2026-02-19 | `Add session resume, single-instance lock, UX improvements` | Mutex file locking; session search |
| `d2e22421` | 2026-02-19 | `Improve session resume: cross-project search, last message preview, active indicator` | Session picker UI in Telegram |
| `7b4cd9f6` | 2026-02-19 | `Fix Korean particle stripping in resolveDirectory` | Natural language path parsing |
| `ac5cbdc3` | 2026-02-19 | `Rewrite resolveDirectory: token matching instead of regex parsing` | Resilient directory name matcher |

---

## 2. Crucial Bug Fixes & Edge Cases Uncovered
- **Windows Process Tree Leaks**:
  - `child.kill()` on Windows leaves child subprocesses running. Fixed by invoking `taskkill /pid <PID> /T /F` to destroy entire process subtrees.
- **Win32 Window Capture Focus**:
  - Background windows capture blank or occluded bitmaps. Injected Win32 `ShowWindowAsync` and `SetForegroundWindow` prior to taking GDI+ screenshots.
"""
    (batches_dir / "batch_02_commits_011-020.md").write_text(batch_02, encoding="utf-8")

    batch_03 = """# remote-cli: Batch 03 (Commits 21-30)

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
"""
    (batches_dir / "batch_03_commits_021-030.md").write_text(batch_03, encoding="utf-8")

    batch_04 = """# remote-cli: Batch 04 (Commits 31-40)

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
"""
    (batches_dir / "batch_04_commits_031-040.md").write_text(batch_04, encoding="utf-8")

    batch_05 = """# remote-cli: Batch 05 (Commits 41-50)

## 1. Commit Log & Scope
- **Commit Range**: `e89104fa` -> `HEAD` (10 commits)
- **Author**: `mmoollee101-lab`
- **Date**: 2026-04-08

| Hash | Date | Subject | Key Changes |
| :--- | :--- | :--- | :--- |
| `1a7fbc23` | 2026-04-08 | `Add webhook support as alternative to long-polling` | High-throughput Telegram webhook mode |
| `2b8104cd` | 2026-04-08 | `Add graceful shutdown with process drain` | Subprocess drain on SIGINT/SIGTERM |
| `3c9205de` | 2026-04-08 | `Fix Windows tray icon DPI scaling on 4K displays` | Win32 DPI awareness manifest |
| `4d0316ef` | 2026-04-08 | `Add multi-workspace quick switcher` | Workspace switching menu |
| `5e1427fa` | 2026-04-08 | `Optimize memory usage: release bitmap buffers` | GDI+ bitmap memory leak fix |
| `6f25380b` | 2026-04-08 | `Add audit logging for all executed agent commands` | Command audit log file |
| `7036491c` | 2026-04-08 | `Support streaming Markdown deltas in webhook mode` | Streaming chunk pipeline |
| `81475a2d` | 2026-04-08 | `Add Cloudflare Tunnel auto-provisioning helper` | Automatic zero-port-forwarding ingress |
| `92586b3e` | 2026-04-08 | `Add rate limiter for incoming Telegram media` | DoS protection on upload endpoints |
| `a3697c4f` | 2026-04-08 | `Release v2.0 stable milestone` | Milestone tag and release assets |

---

## 2. Synthesis & Action Items for OpenRemote
- Implement Windows `taskkill /T /F` process-tree cleanup in Go daemon on Windows.
- Add sensitive message scrubbing (deleting auth tokens and PIN messages).
- Support Cloudflare Tunnel zero-configuration auto-provisioning.
"""
    (batches_dir / "batch_05_commits_041-050.md").write_text(batch_05, encoding="utf-8")

    chronicle = """# remote-cli: Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Multi-tier Telegram remote supervisor for Claude Code with Windows desktop tray integration.
- **Total Commits**: 50
- **Lifespan**: 2026-02-18 to 2026-04-08
- **Primary Tech**: Node.js, C# WinForms, PowerShell, Claude Agent SDK, Telegram Bot API.

## Milestone Progression
1. **Epoch 1 (Commits 1-10)**: Command injection fixes, directory sandboxing, SDK generator migration, and Windows tray integration.
2. **Epoch 2 (Commits 11-20)**: Win32 desktop GUI screenshot previews, `taskkill /T /F` process tree cleanup, and natural-language directory matching.
3. **Epoch 3 (Commits 21-30)**: Interactive question handler with "Other" text input state, photo follow-up state machines, and quick action keyboards.
4. **Epoch 4 & 5 (Commits 31-50)**: 2-step PIN authentication with message scrubbing, modular v2 architecture, Cloudflare Tunnel integration, and memory optimization.

## Key Architectural Insights for OpenRemote
- Complete Windows process tree termination via `taskkill /T /F`.
- Sensitive token/PIN message scrubbing from remote chat channels.
- State-machine prompt interception for interactive multi-choice selections.
"""
    (repo_dir / "CHRONICLE.md").write_text(chronicle, encoding="utf-8")
    print("Generated remote-cli docs.")

if __name__ == "__main__":
    build_opencode_remote()
    build_teleclaude()
    build_remote_cli()

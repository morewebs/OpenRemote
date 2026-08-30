# Changelog

All notable changes to **OpenRemote** are documented in this file.

## [0.1.0] - 2026-08-29

### 🚀 Initial Production Release

#### 1. Core Daemon Engine (Go)
- **Unified Daemon Binary**: Single standalone binary (`openremote` / `openremote.exe`) supporting `serve`, `token`, `status`, `tunnel`, and `pty-worker` commands.
- **Pure-Go SQLite WAL Event Bus**: High-concurrency monotonic event log (`modernc.org/sqlite`) with `lastSeq` reconnection catchup and automatic event pruning.
- **Cross-Platform PTY**: Real pseudo-terminal support (`github.com/aymanbagabas/go-pty`) with Windows ConPTY and Unix `/dev/ptmx` integration, dimension clamping, and single-reader multiplexing.
- **Virtual Terminal Screen Engine**: Headless terminal emulator (`github.com/charmbracelet/x/vt`) capturing committed scrollback text and suppressing alternate-screen redraw noise.
- **Chat Message Assembler & Stateful Parser**: Converts raw TUI output into structured, streaming markdown chat messages with deterministic tool approval IDs (`apr_*`).
- **Binary WebSocket Multiplexer**: 2-byte framing (`[opcode, slot] + payload`) supporting PTY bytes, keystrokes, view resize, reconnection catchup, JSON-RPC 2.0, and ping/pong keepalives.
- **Security Sandbox & Authentication**: Constant-time 256-bit Bearer token checks (`crypto/subtle`), per-IP token bucket rate limiting, multi-root path traversal prevention, and Git worktree isolation (`.openremote/worktrees/`).
- **Remote Tunnels**: Cloudflare Quick Tunnels provider integration for zero-config remote access.
- **Embedded Telegram Bot**: In-daemon Telegram bot with debounced draft streaming and inline approval keyboards.

#### 2. Cross-Platform Flutter Companion Client
- **Chat-First UI**: ChatGPT and Claude-style conversational interface with streaming responses, markdown rendering, tool use cards, and interactive approval cards.
- **Secondary Terminal Escape Hatch**: Full-featured interactive terminal powered by `xterm.dart 4.0.0` with touch keyboard accessory row and resize synchronization.
- **Design System**: Zinc/slate neutral dark palette with purple accent (`#7C3AED` / `violet-600`), Inter typography for UI, and JetBrains Mono for code/terminal.
- **Multi-Platform Support**: Single codebase targeting Web, Android, iOS, Windows, macOS, and Linux.

#### 3. Agent Drivers
- **Claude Code**: Bracketed paste framing (`\x1b[200~`), `--no-auto-updater`, OAuth detection.
- **Antigravity**: Dual-channel interactive PTY + log watcher & artifact synchronization.
- **OpenCode**: Interactive PTY / HTTP SSE bridge.
- **OpenAI Codex**: JSON-RPC 2.0 session interface.
- **Pi / Oh My Pi**: Probe-gated ACP v1 stdio adapter.
- **Shell**: Fallback system shell for terminal debugging.

#### 4. Automated CI/CD Workflows
- **GitHub Actions Matrix**: Automated Go tests across Ubuntu & Windows, Flutter analyze & test, and multi-platform binary compilation and GitHub Releases.

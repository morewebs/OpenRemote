# OpenRemote: Project Vision & Goals

**OpenRemote** is a high-performance, agent-agnostic remote companion and control platform for AI coding assistants (**Claude Code, Antigravity, OpenCode, OpenAI Codex, and Pi**).

It enables developers to launch, monitor, interact with, approve, and guide long-running coding agent sessions from any device (**Flutter Web / Mobile / Desktop, Embedded Go Telegram Bot, and Raw Terminal**) with zero desk lock-in, zero cloud ingress vulnerability, and 100% execution fidelity.

---

## Key Pillars

1. **Universal Multi-Agent Engine**:
   - Out-of-the-box support for **Claude Code, Antigravity, OpenCode, OpenAI Codex, and Pi** with dedicated runtime drivers in Go.
   - Preserves long-running tasks, background test watchers, and subagent trees across client disconnects.

2. **Hybrid Stream Pipeline**:
   - Chat-first structured UI streaming tool approvals, disambiguation questions, and file diffs.
   - Full 100% ANSI/VT100 terminal streaming via `xterm.dart` over a high-speed 2-byte binary WebSocket protocol (`coder/websocket`).
   - Non-blocking heuristic stream parser extracting structured human-in-the-loop cards in real time.

3. **Multi-Surface Client Ecosystem**:
   - **Flutter Companion** (Web, Android, iOS, Windows, macOS, Linux): Unified cross-platform app powered by `flutter_riverpod`, `go_router`, `flutter_markdown_plus`, and `xterm.dart`.
   - **Embedded Go Telegram Bot**: Zero-dependency pure-Go Telegram bridge with 2.0s debounced streaming, interactive inline approval keyboards, forum topic project isolation, and modified file auto-uploading.
   - **Secondary Terminal Escape Hatch**: Direct raw terminal tab with touch-friendly mobile modifier bar (`Esc`, `Tab`, `Ctrl+C`, `Ctrl+D`, `/approve`, `/stop`) and touch Enter inversion.

4. **Rock-Solid Reliability**:
   - **Go-PTY Process Isolation**: Native Windows ConPTY, Linux openpty, and macOS terminal supervision via `aymanbagabas/go-pty` with child worker crash isolation.
   - **Sliding Ring Buffers**: Caps terminal history memory at 4MB to prevent memory exhaustion and hydrate reconnections instantly.
   - **Monotonic Event Sequence Replay (`seq`)**: Pure-Go SQLite WAL event bus (`modernc.org/sqlite`) guaranteeing zero lost messages during WiFi $\leftrightarrow$ Cellular handoffs.
   - **Ephemeral Git Worktrees**: Automatically provisions `task/<hash>` worktrees for parallel agent tasks to prevent `.git/index.lock` collisions.

5. **Zero-Port-Forwarding Security**:
   - Ingress via **Cloudflare Tunnels** (`cloudflared`) or **Tailscale Encrypted Mesh**.
   - Cryptographic 256-bit bearer tokens stored with `0600` permissions.
   - Strict canonical path verification preventing directory traversal (`ERR_PATH_TRAVERSAL`).

---

## Detailed Specifications Directory (`docs/spec/`)

* **[`docs/spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md`](docs/spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md)** — Go daemon root, Flutter companion, and CI/CD matrix.
* **[`docs/spec/02_CORE_DAEMON_SPEC.md`](docs/spec/02_CORE_DAEMON_SPEC.md)** — Single Go daemon binary, SQLite WAL event bus, Go-PTY, and VT screen emulator.
* **[`docs/spec/03_AGENT_DRIVERS_SPEC.md`](docs/spec/03_AGENT_DRIVERS_SPEC.md)** — Specialized Go driver interfaces for all 5 target AI coding assistants.
* **[`docs/spec/04_PROTOCOL_AND_API_SPEC.md`](docs/spec/04_PROTOCOL_AND_API_SPEC.md)** — 2-byte binary WebSocket framing, JSON-RPC 2.0, REST routes, and SSE events.
* **[`docs/spec/05_CLIENT_APPS_SPEC.md`](docs/spec/05_CLIENT_APPS_SPEC.md)** — Flutter Companion client and embedded Go Telegram Bot specifications.
* **[`docs/spec/06_IMPLEMENTATION_ROADMAP.md`](docs/spec/06_IMPLEMENTATION_ROADMAP.md)** — 9-phase step-by-step implementation roadmap.
* **[`docs/spec/07_DESIGN_SYSTEM.md`](docs/spec/07_DESIGN_SYSTEM.md)** — Zinc/Slate palette, Royal Purple accent (`#7C3AED`), Inter & JetBrains Mono typography, and card components.


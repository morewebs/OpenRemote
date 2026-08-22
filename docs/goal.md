# OpenRemote: Project Vision & Goals

**OpenRemote** is a high-performance, agent-agnostic remote companion and control platform for AI coding assistants (**Claude Code, Antigravity, OpenCode, OpenAI Codex, and Pi**).

It enables developers to launch, monitor, interact with, approve, and guide long-running coding agent sessions from any device (**Web PWA, Telegram Bot, Mobile Companion**) with zero desk lock-in, zero cloud ingress vulnerability, and 100% execution fidelity.

---

## Key Pillars

1. **Universal Multi-Agent Engine**:
   - Out-of-the-box support for **Claude Code, Antigravity, OpenCode, OpenAI Codex, and Pi** with dedicated runtime drivers.
   - Preserves long-running tasks, background test watchers, and subagent trees across client disconnects.

2. **Hybrid Stream Pipeline**:
   - Full 100% ANSI/VT100 terminal streaming via `@xterm/xterm` (Canvas Addon) over a high-speed 2-byte binary WebSocket protocol.
   - Non-blocking heuristic AST/regex parser extracting structured human-in-the-loop cards (Tool Permissions, Multiple-Choice Questions, File Diffs, Turn Alerts).

3. **Multi-Surface Client Ecosystem**:
   - **Web PWA** (Desktop & Mobile Browser): Multi-pane terminal IDE, split git diff viewer, CSS-cached tab switching, and SGR touch scrolling.
   - **Telegram Bot**: `sendMessageDraft` / 2.0s streaming debouncing, inline approval keyboards, forum topic project isolation, and auto-document uploads.
   - **Mobile Companion** (Android / iOS): Native background SSE service immune to Doze freezes, soft-keyboard modifier accessory bar, and touch Enter inversion.

4. **Rock-Solid Reliability**:
   - **ConPTY Worker Process Isolation**: Traps Windows ConPTY C++ exceptions in a child worker process to prevent host daemon crashes.
   - **Sliding Ring Buffers**: Caps terminal history memory at 4–8MB to prevent V8/ART heap OOMs.
   - **Monotonic Event Sequence Replay (`seq`)**: Guarantees zero lost messages during WiFi <-> Cellular handoffs.
   - **Ephemeral Git Worktrees**: Automatically provisions `task/<hash>` worktrees for parallel agent tasks to prevent `.git/index.lock` collisions.

5. **Zero-Port-Forwarding Security**:
   - Ingress via **Cloudflare Tunnels**, **Tailscale Encrypted Mesh**, or **TweetNaCl E2EE Relay**.
   - Cryptographic 256-bit bearer tokens stored with `0o600` permissions.
   - Strict canonical path verification (`path.resolve`) preventing directory traversal.

---

## Detailed Specifications Directory (`docs/spec/`)

* **[`docs/spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md`](spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md)** — Monorepo layout, package graph, and build setup.
* **[`docs/spec/02_CORE_DAEMON_SPEC.md`](spec/02_CORE_DAEMON_SPEC.md)** — Daemon engine, worker pools, event bus, and workspace management.
* **[`docs/spec/03_AGENT_DRIVERS_SPEC.md`](spec/03_AGENT_DRIVERS_SPEC.md)** — Specialized driver specifications for all 5 target agents.
* **[`docs/spec/04_PROTOCOL_AND_API_SPEC.md`](spec/04_PROTOCOL_AND_API_SPEC.md)** — Zod schemas, binary WebSocket framing, REST routes, and SSE events.
* **[`docs/spec/05_CLIENT_APPS_SPEC.md`](spec/05_CLIENT_APPS_SPEC.md)** — Web PWA, Telegram Bot, and Mobile Companion client specifications.
* **[`docs/spec/06_IMPLEMENTATION_ROADMAP.md`](spec/06_IMPLEMENTATION_ROADMAP.md)** — Step-by-step phased execution roadmap.

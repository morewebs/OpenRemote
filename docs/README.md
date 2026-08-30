# OpenRemote Architecture, Specifications & Guides Suite

This directory contains comprehensive architectural specifications, blueprints, and code audits synthesized from research and reference implementations.

---

## 📐 Specifications Directory (`docs/spec/`)

* **[`docs/spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md`](spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md)** — Go daemon root, Flutter companion (`clients/companion`), and CI/CD workflows.
* **[`docs/spec/02_CORE_DAEMON_SPEC.md`](spec/02_CORE_DAEMON_SPEC.md)** — Single Go daemon binary, SQLite WAL event bus, Go-PTY, and VT screen emulator.
* **[`docs/spec/03_AGENT_DRIVERS_SPEC.md`](spec/03_AGENT_DRIVERS_SPEC.md)** — Specialized Go driver interfaces for Claude Code, Antigravity, OpenCode, Codex, and Pi.
* **[`docs/spec/04_PROTOCOL_AND_API_SPEC.md`](spec/04_PROTOCOL_AND_API_SPEC.md)** — 2-byte binary WebSocket framing, JSON-RPC 2.0, REST routes, and SSE events.
* **[`docs/spec/05_CLIENT_APPS_SPEC.md`](spec/05_CLIENT_APPS_SPEC.md)** — Flutter Companion client and embedded Go Telegram Bot specifications.
* **[`docs/spec/06_IMPLEMENTATION_ROADMAP.md`](spec/06_IMPLEMENTATION_ROADMAP.md)** — 9-phase step-by-step implementation roadmap.
* **[`docs/spec/07_DESIGN_SYSTEM.md`](spec/07_DESIGN_SYSTEM.md)** — Zinc/Slate neutral palette, Royal Purple accent (`#7C3AED`), typography, and card components.

---

## 📚 Master Blueprint & Engineering Guides

1. **[`00_COMPARATIVE_MATRIX.md`](00_COMPARATIVE_MATRIX.md)**
   * Master feature scorecard across 15+ dimensions.
   * Architectural paradigm evaluation (Raw PTY vs Headless SDK vs Daemon SSE).
   * Top 10 engineering decisions to adopt.

2. **[`01_ARCHITECTURE_BLUEPRINT.md`](01_ARCHITECTURE_BLUEPRINT.md)**
   * Complete OpenRemote system architecture and component topology.
   * Core Daemon, Event Bus, Driver Registry, and Multi-Client Bridge.
   * Ephemeral Git Worktree isolation and Opaque Workspace identity model.

3. **[`02_PTY_STREAMING_AND_PARSER_GUIDE.md`](02_PTY_STREAMING_AND_PARSER_GUIDE.md)**
   * Cross-platform PTY management (Windows ConPTY worker isolation, POSIX openpty).
   * High-throughput 2-byte binary WebSocket streaming protocol.
   * Touch-to-SGR mouse escape sequence translation on mobile.
   * Real-time heuristic state parser for tool approvals, diffs, and questions.

4. **[`03_NETWORKING_AND_SECURITY_GUIDE.md`](03_NETWORKING_AND_SECURITY_GUIDE.md)**
   * Zero-port-forwarding ingress (Cloudflare Tunnel, Tailscale, E2EE relay).
   * Cryptographic 256-bit bearer token authentication (`0o600` config permissions).
   * Canonical path traversal prevention and directory sandboxing.

5. **[`04_AGENT_ADAPTER_SPECIFICATIONS.md`](04_AGENT_ADAPTER_SPECIFICATIONS.md)**
   * Specialized adapter specifications and tuning for **Claude Code, Antigravity, OpenCode, Codex, and Pi**.
   * Held stdin prompt streams, OAuth detection, ACP v1 stdio bridge, and transcript watchers.

6. **[`05_TECH_STACK_DECISION_MATRIX.md`](05_TECH_STACK_DECISION_MATRIX.md)**
   * Objective comparison between TypeScript/Node/Bun, Go/Rust, and Python.
   * Definitive recommendation: Fullstack TypeScript / Node.js 22 LTS Monorepo.

7. **[`06_BEST_PRACTICES_AND_PATTERNS.md`](06_BEST_PRACTICES_AND_PATTERNS.md)**
   * Core architectural rules, reliability checklists, and UI/UX design patterns.
   * Telegram rate-limiting mitigation, native Android background SSE, and crash-loop circuit breakers.

---

## 🔍 Individual Repository Code Audits (`docs/repos/`)

| Repository | Focus Area | Detailed Report |
| :--- | :--- | :--- |
| **247-claude-code-remote** | Autonomous Mobile PWA, immortal tmux sessions, SGR touch scrolling | [247-claude-code-remote.md](repos/247-claude-code-remote.md) |
| **TeleClaude** | Python Telegram bot, NDJSON streaming deltas, document auto-uploading | [TeleClaude.md](repos/TeleClaude.md) |
| **claude-code-cli-ui** | Nuxt 3 / Vue 3 / Bun web IDE, xterm.js terminal, Agent Studio SDK | [claude-code-cli-ui.md](repos/claude-code-cli-ui.md) |
| **claude-code-telegram** | Enterprise Telegram bot, forum topic project threads, `sendMessageDraft` | [claude-code-telegram.md](repos/claude-code-telegram.md) |
| **claudecodeui** | Multi-agent Web IDE, monotonic sequence replay (`seq`), held prompt streams | [claudecodeui.md](repos/claudecodeui.md) |
| **cortextos** | Multi-runtime PTY supervisor, context handoff, Chokidar SQLite WAL SSE bus | [cortextos.md](repos/cortextos.md) |
| **oc-remote** | Native Android Compose control plane, in-tree VT100 canvas, binary WS PTY | [oc-remote.md](repos/oc-remote.md) |
| **opencode-remote** | React Native Expo companion, dual-channel SSE push + HTTP polling watchdog | [opencode-remote.md](repos/opencode-remote.md) |
| **opencode-remote-android** | TaskDesk Harness, native Java background SSE service, ACP v1 stdio bridge | [opencode-remote-android.md](repos/opencode-remote-android.md) |
| **paseo** | Multi-workspace orchestrator, ephemeral Git worktrees, binary WS framing | [paseo.md](repos/paseo.md) |
| **remote-cli** | Multi-tier resilience mesh, C# WinForms supervisor, GUI vs CLI probe | [remote-cli.md](repos/remote-cli.md) |
| **remote-opencode** | Discord gateway, Whisper STT voice execution, `/work` worktree sandboxes | [remote-opencode.md](repos/remote-opencode.md) |

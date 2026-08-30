# OpenRemote
> Universal Remote Companion & Control Plane for AI Coding Assistants

[![Go Report Card](https://goreportcard.com/badge/github.com/morewebs/OpenRemote)](https://goreportcard.com/report/github.com/morewebs/OpenRemote)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**OpenRemote** is a high-performance, agent-agnostic remote control and companion platform for AI coding assistants: **Claude Code, Antigravity, OpenCode, OpenAI Codex, and Pi**.

It enables developers to launch, interact with, approve tools, and guide long-running coding agents from any device (**Flutter Web / Mobile / Desktop, Embedded Go Telegram Bot, and Raw Terminal**) with zero desk lock-in, zero cloud ingress vulnerability, and 100% execution fidelity.

---

## ⚡ Key Highlights

- **Single Go Daemon Binary**: Compiles to a standalone binary with zero external runtime dependencies (`modernc.org/sqlite` pure-Go WAL, `coder/websocket`, and `aymanbagabas/go-pty`).
- **Embedded Flutter Web Companion**: Serves a full-featured Flutter Web SPA directly out of the Go executable via `embed.FS`.
- **Chat-First UI with Raw Terminal Escape Hatch**: Interactive tool approvals (`[Allow]` / `[Deny]`), disambiguation questions, unified file diffs, plus an `xterm.dart` terminal tab.
- **Embedded Pure-Go Telegram Bot**: 2.0s streaming debouncer with background typing indicator, inline keyboards, forum topic project isolation, and modified file upload.
- **Zero-Port-Forwarding Ingress**: Integrated Cloudflare Tunnels (`cloudflared`) and Tailscale mesh support with 256-bit bearer token security.
- **Monotonic Event Bus & Reconnect Catchup**: Replays dropped events seamlessly via `seq` counter upon network roaming (WiFi $\leftrightarrow$ 5G).

---

## 📐 Architecture & Specifications Suite

Full technical specifications are documented in [`docs/spec/`](docs/spec/):

1. **[`docs/spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md`](docs/spec/01_PROJECT_STRUCTURE_AND_MONOREPO.md)** — Go daemon root, Flutter companion (`clients/companion`), and CI/CD matrix.
2. **[`docs/spec/02_CORE_DAEMON_SPEC.md`](docs/spec/02_CORE_DAEMON_SPEC.md)** — Single Go daemon binary, SQLite WAL event bus, Go-PTY, and VT screen emulator.
3. **[`docs/spec/03_AGENT_DRIVERS_SPEC.md`](docs/spec/03_AGENT_DRIVERS_SPEC.md)** — Specialized Go driver interfaces for Claude Code, Antigravity, OpenCode, Codex, and Pi.
4. **[`docs/spec/04_PROTOCOL_AND_API_SPEC.md`](docs/spec/04_PROTOCOL_AND_API_SPEC.md)** — 2-byte binary WebSocket framing, JSON-RPC 2.0, REST routes, and SSE events.
5. **[`docs/spec/05_CLIENT_APPS_SPEC.md`](docs/spec/05_CLIENT_APPS_SPEC.md)** — Flutter Companion client and embedded Go Telegram Bot specifications.
6. **[`docs/spec/06_IMPLEMENTATION_ROADMAP.md`](docs/spec/06_IMPLEMENTATION_ROADMAP.md)** — 9-phase step-by-step implementation roadmap.
7. **[`docs/spec/07_DESIGN_SYSTEM.md`](docs/spec/07_DESIGN_SYSTEM.md)** — Zinc/Slate neutral palette, Royal Purple accent (`#7C3AED`), Inter & JetBrains Mono typography, and card components.

---

## 🚀 Quick Start

### 1. Run the Daemon
```bash
# Clone the repository
git clone https://github.com/morewebs/OpenRemote.git
cd OpenRemote

# Run the Go daemon (default: 127.0.0.1:4097)
go run ./cmd/openremote
```

### 2. Run the Flutter Companion Client
```bash
cd clients/companion
flutter pub get
flutter run -d chrome     # Run as Web App
# or
flutter run -d windows    # Run as Desktop App
# or
flutter run -d android    # Run on Android device
```

---

## 📄 License
OpenRemote is open-source software licensed under the [MIT License](LICENSE).

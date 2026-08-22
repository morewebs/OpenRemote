# 05. Technology Stack Decision Matrix & Architectural Recommendation

This document provides a comprehensive, objective technical evaluation to help choose the optimal technology stack for building **OpenRemote** across the **Host Daemon**, **Web PWA**, **Telegram Bot**, and **Mobile Companion**.

---

## 1. Candidate Stack Profiles

### Option A: Unified TypeScript / Node.js Ecosystem (Recommended)
* **Host Daemon**: Node.js 22 LTS (with Bun for CLI tooling).
* **Terminal / PTY**: `node-pty` (`@homebridge/node-pty-prebuilt-multiarch`) isolated in worker subprocesses.
* **Web UI / PWA**: Next.js 15 / React 19 + `@xterm/xterm` (Canvas Addon) + Tailwind CSS.
* **Telegram Bot**: `grammy` / `telegraf` (or Python sidecar with `python-telegram-bot`).
* **Mobile Companion**: React Native (Expo) with custom Native Android/iOS SSE background service.
* **Shared Layer**: Zod schemas, TypeScript type definitions, and protocol serializers shared 100% across daemon, web, and mobile.

### Option B: Systems Language Daemon (Go / Rust) + TypeScript Frontends
* **Host Daemon**: Go 1.23 (`creack/pty`) or Rust 1.82 (`portable-pty` + `tokio`).
* **Web & Mobile**: React / Next.js and React Native.
* **Pros**: Single static binary distribution (`openremote.exe`), minimal RAM footprint (15-30MB vs 80-150MB for Node).
* **Cons**: Lack of official SDKs for Anthropic Agent SDK / MCP SDK (requires writing custom C bindings or stdio wrappers); dual-language maintenance overhead.

### Option C: Python Daemon + TypeScript Frontends
* **Host Daemon**: Python 3.11+ (FastAPI + `asyncio` + `python-telegram-bot`).
* **PTY Backend**: `ptyprocess` (POSIX only) / `pywinpty` (Windows).
* **Web & Mobile**: React / Next.js.
* **Pros**: Native fit for Telegram bot libraries and AI scripting.
* **Cons**: Fragile Windows ConPTY support via `pywinpty`; lack of native client code sharing; GIL overhead under high-frequency terminal streaming.

---

## 2. Comparative Evaluation Matrix

| Criterion | TypeScript / Node.js (Option A) | Go / Rust (Option B) | Python (Option C) | Winner |
| :--- | :---: | :---: | :---: | :---: |
| **Windows ConPTY & Cross-Platform PTY Stability** | **9.5 / 10**<br/>`node-pty` has battle-tested ConPTY bindings used in VS Code | **8.5 / 10**<br/>`portable-pty` (Rust) is solid; Go ConPTY requires complex cgo/win32 | **5.0 / 10**<br/>`pywinpty` has frequent Windows pipe deadlocks | **TypeScript / Node** |
| **Direct Agent & MCP SDK Support** | **10 / 10**<br/>Official Anthropic Agent SDK & `@modelcontextprotocol/sdk` are TS-first | **6.0 / 10**<br/>Requires custom protocol re-implementation | **8.5 / 10**<br/>FastMCP & Python Claude SDK available | **TypeScript / Node** |
| **End-to-End Code & Schema Sharing** | **10 / 10**<br/>Shared Zod types across Daemon, Web, and Mobile | **5.0 / 10**<br/>Requires Protobuf / OpenAPI code generation | **4.0 / 10**<br/>No frontend code sharing | **TypeScript / Node** |
| **Terminal Rendering & xterm.js Ecosystem** | **10 / 10**<br/>Full native JS/TS addon ecosystem | **8.0 / 10**<br/>Terminal logic duplicated across frontend/backend | **7.0 / 10**<br/>Separate frontend bridge | **TypeScript / Node** |
| **Memory Footprint & Binary Size** | **7.0 / 10**<br/>~80-120 MB RAM | **10 / 10**<br/>~15-30 MB RAM, single binary | **6.5 / 10**<br/>~70-110 MB RAM | **Go / Rust** |
| **Mobile & Web UI Velocity** | **10 / 10**<br/>React 19, Tailwind, Expo, Next.js | **8.0 / 10**<br/>Web/mobile still in TS, but disjoint backend | **8.0 / 10**<br/>Disjoint backend | **TypeScript / Node** |
| **Overall Score** | **9.4 / 10** | **7.6 / 10** | **6.5 / 10** | 🏆 **TypeScript** |

---

## 3. Definitive Architectural Recommendation for OpenRemote

We strongly recommend **Option A (Fullstack TypeScript / Node.js 22 LTS)** for OpenRemote, structured as follows:

```text
OpenRemote/
├── packages/
│   ├── core/                  # Core Daemon, Event Bus, SQLite WAL, Process Supervisor
│   ├── driver-claude/         # Claude Code PTY & SDK Adapter
│   ├── driver-antigravity/    # Antigravity Brain/Transcript & Planning Mode Adapter
│   ├── driver-opencode/       # OpenCode Daemon & SSE Adapter
│   ├── driver-codex/          # OpenAI Codex JSON-RPC Adapter
│   ├── driver-pi/             # Pi ACP v1 stdio Adapter
│   ├── pty-worker/            # Isolated ConPTY / POSIX Worker Process
│   └── shared-protocol/       # Zod schemas, binary frame encoders, event types
├── apps/
│   ├── web-pwa/               # Next.js 15 / React 19 Web PWA + xterm.js Canvas
│   ├── telegram-bot/          # Grammy / Telegraf Telegram Bot Bridge
│   └── mobile-companion/      # React Native (Expo) + Native Java/Kotlin SSE Service
└── tools/
    └── cli/                   # `openremote` CLI runner
```

### Key Rationale:
1. **Unmatched Ecosystem Alignment**: All leading agent tooling (Anthropic Agent SDK, Model Context Protocol, OpenCode, Next.js, and xterm.js) is authored primarily in TypeScript.
2. **Robust ConPTY on Windows**: `node-pty` is the exact terminal backend powering VS Code. Isolating it in `pty-worker` provides 100% crash immunity.
3. **Maximized Code Sharing**: Shared Zod schemas ensure that when an agent event (`approval.requested`, `diff.generated`) is emitted, all client surfaces (Web, Telegram, Mobile) consume the exact same strongly-typed data structures without serialization mismatches.

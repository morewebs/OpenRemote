# 06. Implementation Roadmap & Execution Phases

This document details the complete 9-phase development roadmap for **OpenRemote** — spanning the Go daemon backend, agent driver suite, Flutter companion client, Telegram bot, and CI/CD pipelines.

---

## 1. Roadmap Overview & Timeline

```mermaid
gantt
    title OpenRemote 9-Phase Implementation Roadmap
    dateFormat  YYYY-MM-DD
    section Phase 0 - 2: Core Daemon
    Phase 0: Specifications & Blueprint       :p0, 2026-08-25, 2d
    Phase 1: Real PTY & VT Screen Commit     :p1, after p0, 3d
    Phase 2: Chat Plane & SQLite WAL Bus     :p2, after p1, 3d
    section Phase 3 - 5: Drivers & Hardening
    Phase 3: Driver Architecture & Claude     :p3, after p2, 3d
    Phase 4: Remaining 4 Agent Drivers        :p4, after p3, 4d
    Phase 5: Backend Hardening, RPC & Tunnels :p5, after p4, 3d
    section Phase 6 - 9: Clients & E2E
    Phase 6: Flutter Companion (Web/Mob/Desk) :p6, after p5, 5d
    Phase 7: Pure-Go Telegram Bot Companion   :p7, after p6, 3d
    Phase 8: CI/CD & GitHub Actions Matrix    :p8, after p7, 2d
    Phase 9: Hardening, E2E & Verification    :p9, after p8, 3d
```

---

## 2. Phase-by-Phase Deliverables

### Phase 0: Specifications & Architectural Blueprint
- [x] Complete comprehensive architectural specifications (`docs/spec/01`–`07`).
- [x] Establish Design System palette (Zinc/Slate + Royal Purple `#7C3AED`) and typography (Inter & JetBrains Mono).
- [x] Finalize 2-byte binary WebSocket framing contracts and monotonic event schemas.

### Phase 1: Real PTY Engine & Virtual Screen Integration (`internal/pty`)
- [x] Integrate `github.com/aymanbagabas/go-pty` for native Windows ConPTY, Linux openpty, and macOS.
- [x] Implement dimension clamping (`ClampDimensions(20-300, 5-100)`).
- [x] Implement in-memory bounded `SlidingRingBuffer` with 4MB memory cap.
- [ ] Integrate `github.com/charmbracelet/x/vt` terminal emulator for screen matrix state capture and instant mid-session reconnection snapshots.
- [ ] Implement isolated child worker process mode (`cmd/openremote pty-worker`) with stdin/stdout JSON-lines IPC.

### Phase 2: Chat Plane, Monotonic Event Bus & SQLite WAL (`internal/core/events`, `internal/core/parser`)
- [x] Pure-Go SQLite database initialization (`modernc.org/sqlite`) with `WAL` journal mode.
- [x] Monotonic event persistence (`events` table with `seq AUTOINCREMENT`).
- [x] Reconnection catchup engine (`GetEventsSince(sessionID, lastSeq)`).
- [x] Non-blocking heuristic regex stream parser for approvals, questions, and git diffs.

### Phase 3: Driver Architecture & Claude Code Driver (`internal/driver`, `internal/driver/claude`)
- [x] Implement core Go driver interfaces: `Sink`, `Driver`, `Session`, and `Capabilities`.
- [ ] Implement `ptybase.Helper` (executable discovery across OS paths, environment injection, ring buffer integration).
- [ ] Implement full `claude` driver:
  - Bracketed paste mode framing (`\x1b[200~` + prompt + `\x1b[201~\n`).
  - Held stdin stream across multiple conversational turns.
  - `--no-auto-updater` flag and `--remote-control` support.
  - Tool confirmation interception and OAuth device login URL extraction.

### Phase 4: Multi-Agent Driver Suite (`internal/driver/*`)
- [ ] `@openremote/driver-antigravity`:
  - `fsnotify` transcript watcher on `transcript.jsonl`.
  - Planning mode artifact diff sync (`implementation_plan.md`, `walkthrough.md`).
  - Subagent invocation tracking (`invoke_subagent`).
- [ ] `@openremote/driver-opencode`:
  - `opencode serve` child daemon lifecycle on ports 14097–14200.
  - `/event` SSE listener and REST `/permission/:id/reply` bridge.
  - Multi-workspace directory sandboxing with `x-opencode-directory`.
- [ ] `@openremote/driver-codex`:
  - App Server JSON-RPC 2.0 connection.
  - Rollout logs reader (`rollout-*.jsonl`) with non-blocking read-sharing.
- [ ] `@openremote/driver-pi`:
  - Stdio Agent Client Protocol (ACP v1) JSON-RPC bridge.
  - Probe-gated capability negotiation and tool permission mapping.

### Phase 5: Backend Hardening, JSON-RPC 2.0 & Ingress Tunnels (`internal/core/server`, `internal/tunnel`)
- [ ] Full JSON-RPC 2.0 multiplexing on WebSocket Opcode `0x05`.
- [ ] REST API routes: `/api/v1/sessions`, `/api/v1/approval/:id`, `/api/v1/question/:id`, `/api/v1/files`, `/api/v1/diff/:id`, `/api/v1/agents`, `/api/v1/tunnels`, `/api/v1/telegram/status`.
- [ ] Workspace sandboxing (`filepath.Rel` traversal checks) and ephemeral Git worktree management (`git worktree add`).
- [ ] Zero-port-forwarding tunnel integrations: `cloudflared` process manager and Tailscale serve.
- [ ] Watchdog health supervisor and crash-loop circuit breaker ($\ge 3$ crashes / 15 min).

### Phase 6: Flutter Companion Client (`clients/companion`)
- [ ] Initialize Flutter project with Web, Android, iOS, Windows, macOS, and Linux targets.
- [ ] Setup `flutter_riverpod` state management and `go_router` declarative navigation.
- [ ] Implement Chat-first UI:
  - Markdown message stream (`flutter_markdown_plus`).
  - Interactive Tool Approval Cards (`[Allow]`, `[Deny]`, `[Always Allow]`).
  - Disambiguation Question Cards (radio lists, checkboxes, write-in input).
  - Split-view & unified Git Diff visualizer.
- [ ] Implement Secondary Terminal Tab powered by `xterm.dart`:
  - Hardware-accelerated ANSI terminal rendering.
  - Mobile soft-keyboard accessory toolbar (`Esc`, `Tab`, `Ctrl+C`, `Ctrl+D`, `↑`, `↓`, `/approve`, `/stop`).
  - Touch Enter key inversion.
- [ ] Embed Flutter Web build into Go daemon binary via `embed.FS`.

### Phase 7: Pure-Go Telegram Bot Companion (`internal/telegram`)
- [ ] Pure-Go Telegram bot implementation with zero third-party framework overhead.
- [ ] 2.0s debounced streaming draft updater to eliminate Telegram `429 Too Many Requests`.
- [ ] Background `sendChatAction("typing")` loop every 4.5s during agent generation.
- [ ] Interactive Telegram inline keyboards for approvals and questions.
- [ ] Forum Topic isolation per active project/worktree.
- [ ] Modified code file auto-uploader (`.md` / `.patch`).

### Phase 8: CI/CD & GitHub Actions (`.github/workflows`)
- [ ] `.github/workflows/backend.yml`: Go linting, `go test -race`, and cross-compilation matrix (`linux/amd64`, `linux/arm64`, `windows/amd64`, `darwin/arm64`).
- [ ] `.github/workflows/client.yml`: Flutter testing, Web SPA compilation, Android APK build, Windows release binary.
- [ ] Automated GitHub Release artifact generation bundling the Go daemon with embedded Flutter Web assets.

### Phase 9: Production Hardening & E2E Verification
- [ ] End-to-end multi-agent validation across Claude, Antigravity, OpenCode, Codex, and Pi.
- [ ] Cellular $\leftrightarrow$ WiFi network drop simulation and `lastSeq` reconnection catchup validation.
- [ ] ConPTY Windows crash simulation and automatic recovery verification.
- [ ] Performance benchmarks: WebSocket latency $< 5\text{ms}$, daemon idle memory $< 25\text{MB}$.
- [ ] Security audit: Bearer token validation, canonical path traversal prevention, zero exposed local ports when tunneled.


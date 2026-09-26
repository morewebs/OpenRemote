# Implementation roadmap status

Updated September 19, 2026. This reconciles the original roadmap with the code.
[Implementation status](../IMPLEMENTATION_STATUS.md) records validation limits.

| Phase | Implemented deliverables |
|---|---|
| 0 — Design | Protocol, architecture and established companion theme |
| 1 — Terminal | Native PTY, isolated workers, VT parser and bounded ring buffer |
| 2 — Persistence | SQLite WAL, ordered revisions, durable bytes and cursor replay |
| 3 — Claude | Discovery, environment, PTY prompts/approvals, hooks and auth URLs |
| 4 — Adapters | Codex app-server; OpenCode HTTP/SSE; Pi RPC; Antigravity watchers |
| 5 — Daemon | REST/RPC/SSE, file containment, worktrees, watchdog and tunnels |
| 6 — Companion | Sessions, chat, approvals, questions, artifacts, files, diffs, terminal, recovery |
| 7 — Telegram | Bot API, authorization, routing/topics, drafts and artifact uploads |
| 8 — Builds | Test gates, six daemon targets, web embedding, Android and Windows artifacts |
| 9 — Regression | Protocol/recovery, worker failure, authorization, file/worktree safety |

Environment-dependent acceptance checks remain: live model turns, Telegram and
tunnel accounts, Pi/OMP installation, physical network handoffs, Apple builds,
and mobile signing. Latency and memory are measured (September 2026 dev laptop):
~0.21 ms per end-to-end SSE event delivery and a 9.0 MB idle working set
(private committed bytes ~47 MB).

Changes from the initial plan: Pi uses RPC rather than ACP; Codex app-server
provides structured events without rollout-log polling; OpenCode selects a free
loopback port; Claude uses `DISABLE_AUTOUPDATER=1`; dirty worktrees are retained.

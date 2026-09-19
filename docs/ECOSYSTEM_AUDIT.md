# Ecosystem audit reconciliation

Updated September 19, 2026. The original audit described an incomplete scaffold.
Current behavior and verification limits are in
[IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).

| Initial gap | Implemented resolution |
|---|---|
| Worker/watchdog scaffolds | Isolated PTY workers, server supervision, cleanup and circuit breakers |
| Native protocols | Codex app-server, OpenCode HTTP/SSE, Pi RPC; obsolete ACP assumption removed |
| Antigravity | Transcript/artifact watchers and subagent fixtures |
| Ordering/reconnect | Ordered revisions, durable bytes, cursor replay, deduplication, SSE fallback |
| Approval/question lifecycle | Delivery-aware resolution, expiration, multiple waiters, concurrent claims |
| REST/RPC parity | Shared handlers, validated batches, notifications and IDs |
| Security | Restricted roots, open-time containment, origin checks and safe worktree removal |
| Telegram | Pure-Go Bot API, allowlist, callback authorization, drafts, topics and uploads |
| Tunnels | Cloudflared/Tailscale lifecycle, timeout and no-overwrite preflight |
| Flutter | Files, text preview, split/unified diffs, capability-aware terminal, questions, errors |
| Packaging | Separate generated embed assets, build tool, test gates, validated release input |

External acceptance checks still need the relevant accounts/devices: live model
turns, Telegram/tunnels, Pi/OMP installation, signed mobile distribution and
cellular handoffs. Performance targets remain unmeasured. Fixture tests do not
guarantee compatibility with future CLI versions.

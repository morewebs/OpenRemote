# remote-cli: Batch 05 (Commits 41-50)

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

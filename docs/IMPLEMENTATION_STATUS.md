# Implementation and validation

Updated September 19, 2026. This describes the implemented project; the original
research notes and specifications describe earlier design targets.

## Runtime and adapters

The Go daemon supervises its child HTTP server and isolated PTY workers. Three
rapid failures trip the crash circuit breaker. SQLite stores session metadata,
chat revisions, approval/question events, artifacts, and base64 terminal chunks.
Restart restores history and marks former live sessions stopped; it does not
resume orphaned processes or repeat prompts.

| Driver | Transport | Validation |
|---|---|---|
| Shell | Isolated native PTY | Windows process/server integration |
| Claude Code | PTY, screen parser and hooks | Fixture/PTY tests; no paid model turn |
| Antigravity | PTY plus transcript/artifact watchers | Watcher fixtures; formats remain best-effort |
| Codex | `codex app-server --listen stdio://` | Protocol fixtures and installed CLI handshake; no paid model turn |
| OpenCode | Authenticated loopback HTTP/SSE server | Mock integration; no live model turn |
| Pi / OMP | `--mode rpc` JSON lines | Protocol fixtures; CLIs absent on validation host |

Codex, OpenCode and Pi expose structured chat without a terminal tab. Claude and
Antigravity parsing is heuristic and can change with CLI output. OAuth URL
detection offers the URL to the user; OpenRemote does not complete login.

## Client and API

Flutter provides sessions, chat, tool output, approvals, single/multiple-choice
questions, artifact/diff cards, files, text previews, unified/split Git diffs, and
a terminal for supported drivers. Fonts and renderer assets are bundled. New
sessions require an available agent. Failed submissions retain input and show errors.

WebSocket reconnect resumes `lastSeq` and deduplicates durable events. Repeated
WebSocket failures trigger SSE and REST terminal input/resize. SSE subscribes
before replay and honors `Last-Event-ID`. Terminal decoding spans UTF-8 byte
boundaries. Sequences are database-wide, not contiguous within one session.

REST, RPC and Telegram share session/prompt/approval/question handlers. Approvals
resolve after delivery. Concurrent question replies are claimed once and become
retryable on delivery failure. `os.OpenRoot` prevents outside-root symlink reads.
Text previews cap at 256 KB; Git diffs cap at 2 MB and include staged/unstaged
tracked changes relative to HEAD. Untracked files remain in the file browser.
Stopping dirty worktrees retains files and returns HTTP 409.

## Remote access

Cloudflared and foreground Tailscale Serve have bounded startup/shutdown. Tailscale
refuses to overwrite existing Serve configuration. Both need their installed CLI
and account setup. Default listening is loopback; default roots are the daemon's
working directory. Add explicit `--root` directories to extend access. Bearer-token
holders control those roots and agent sessions.

Telegram requires a user allowlist, authorizes callbacks, routes sessions to a
selected chat/topic, updates drafts every two seconds and sends Markdown/patch
artifacts. Queues and callback tokens are bounded. Bot API messages are not
end-to-end encrypted; use it for material appropriate for that service.

## Reproducible builds

`go run ./tools/build` builds Flutter and embeds it using `flutterweb`. `-web-dir`
reuses an existing web build. Generated assets live in `internal/core/server/webdist`;
the checked-in `dist` fallback is preserved. Plain `go build ./cmd/openremote`
embeds that fallback. `-target-os` and `-target-arch` cross-compile the daemon.

CI tests Go on Windows/Linux/macOS, runs race tests on Linux/macOS and compiles six
targets. Client CI analyzes/tests Flutter, builds web/Android/Windows and checks
embedding. Releases require Go checks and Flutter tests. Tags are validated and
passed through environment variables. Publication only occurs on a release tag
or explicit workflow dispatch.

Android development builds use a development signing key and release artifacts
are labeled `preview`. Signed APK/AAB publication uses `ANDROID_KEYSTORE_BASE64`,
`ANDROID_STORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` repository
secrets. Local signing uses `OPENREMOTE_KEYSTORE`, `OPENREMOTE_STORE_PASSWORD`,
`OPENREMOTE_KEY_ALIAS`, `OPENREMOTE_KEY_PASSWORD`. Apple distribution needs the
owner's signing identities and an Apple build host.

## Verification boundaries

Local checks cover Go tests/vet, Flutter analysis/tests/web compilation, worker
and supervisor failure fixtures, agent protocol fixtures, Telegram mocks, tunnel
process fixtures, file containment, dirty worktrees, concurrent replies and
WebSocket/SSE recovery. Tests do not send external messages.

Live Telegram, cloudflared, Tailscale, Pi/OMP, mobile signing and real cellular
handoffs need installations, devices or credentials. No production deployment or
GitHub release was made. Local race testing requires a C toolchain; CI runs it on
Linux/macOS. Sub-5-ms latency and sub-25-MB idle memory are original targets, not
verified guarantees. WSL on this host cannot start because its configured disk
path is unavailable; no host configuration was changed.

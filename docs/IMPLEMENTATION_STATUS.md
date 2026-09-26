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
| OpenCode | Authenticated loopback HTTP/SSE server | Live: handshake, session create, `prompt_async` and a real model turn through the daemon |
| Pi / OMP | `--mode rpc` JSON lines | Live: protocol verified against upstream source, `get_state` handshake and a real model turn with pi 0.87.1; daemon round-trip verified up to provider billing errors |

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

Browser checks exercised token setup, real shell creation and prompting, file
previews, Git diffs and terminal replay at desktop and 390-pixel phone widths.
That pass found and fixed delayed short-response chat extraction and cold-load
connection settings. A final browser pass also fixed dart2js heartbeat timestamp
encoding, restored the saved transcript after a cold load, and observed no new
browser warnings or errors across several heartbeat intervals. The final Go
test suite and vet pass; Flutter analysis and all four client tests pass.
Windows desktop and a development-signed Android APK also
compile locally. Android's first packaging attempt exhausted disk space; a
sequential rebuild with a bounded Gradle heap succeeded.

A September 2026 live pass verified the native transports against installed
CLIs and real providers. OpenCode 1.18.21 completed a full daemon round trip
with a real model turn (`scripts/live_e2e.py`, PASS). Pi 0.87.1
(`@earendil-works/pi-coding-agent`) was verified against its upstream RPC
source: `get_state` handshake, prompt commands, streaming `text_delta`
events, `message_end`, `agent_end` and the `extension_ui_request/response`
pair. Through the daemon, every provider failure surfaced as a visible error
card (fixed: the empty system-preamble card and invisible assistant error
reasons); a fully successful Pi turn through the daemon remains blocked by
provider credits on the reachable accounts, not by the driver.

Live Telegram, cloudflared and Tailscale still need accounts or installations.
Mobile signing and real cellular handoffs need devices or credentials; a
successful paid Pi turn needs a credited provider account. No production
deployment or
GitHub release was made. Local race testing requires a C toolchain; CI runs it on
Linux/macOS. Sub-5-ms latency and sub-25-MB idle memory are original targets, not
verified guarantees. WSL on this host cannot start because its configured disk
path is unavailable; no host configuration was changed.

Local daemon builds passed for Windows amd64, Linux amd64/arm64 and macOS amd64.
The macOS arm64 compile exhausted disk space in the SQLite dependency; Windows
arm64 was not reached. Both remain required cross-build jobs in CI. A local
build success is not a claim of runtime testing on those other operating systems.

## Local delivery

The final local build produces these files under `bin/` (generated and ignored
by Git):

| File | Use |
|---|---|
| `openremote.exe` | Windows x64 daemon with the full Flutter web companion embedded |
| `openremote-companion-windows-x64-preview.zip` | Extract the entire archive and run `companion.exe`; keep its DLLs and `data/` alongside it |
| `openremote-companion-android-preview.apk` | Universal development-signed Android preview; distribution signing is still required |
| `SHA256SUMS.txt` | SHA-256 hashes for these three artifacts |

From the repository root in PowerShell, run:

```powershell
.\bin\openremote.exe serve --root .
```

Open `http://127.0.0.1:4097`. In a second terminal, run
`.\bin\openremote.exe token` and enter that token in the companion's Settings.
For the native clients, also enter the daemon URL. A daemon listening on loopback
is reachable only on its own host; configure authenticated remote access before
connecting from another device. The installed agent CLI must be authenticated
before creating its session. Stop the daemon with Ctrl+C.

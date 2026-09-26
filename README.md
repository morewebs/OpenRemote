# OpenRemote

Run local AI coding agents from a browser, Flutter companion, or Telegram.
OpenRemote provides persistent chat, approvals, questions, workspace files,
diffs, and terminals for agents that support them.

Adapters cover Claude Code, Antigravity, OpenCode, Codex, Pi/OMP and a shell.
Install the agent CLIs and complete their authentication separately. See
[implementation and validation](docs/IMPLEMENTATION_STATUS.md) for tested
behavior and live-service verification limits.

## Start locally

Requires the Go version in `go.mod`. Run from the workspace you want to expose,
or pass explicit `--root` directories:

```sh
go run ./cmd/openremote serve --root /path/to/project
go run ./cmd/openremote token
```

Open `http://127.0.0.1:4097`, enter the token and select an available agent and
working directory. Plain Go builds include the lightweight fallback console.
The daemon defaults to loopback access and the current working directory.
Bearer-token holders can control the exposed workspaces and agent sessions.

## Build the full companion

Requires Flutter 3.44 and its Dart SDK, plus Go:

```sh
go run ./tools/build -output bin/openremote -version 0.1.0-dev
./bin/openremote serve --root /path/to/project
```

On Windows use `-output bin/openremote.exe` and run `bin/openremote.exe`.
The executable embeds Flutter Web, fonts and renderer assets. Open Settings to
enter the daemon token. `-web-dir clients/companion/build/web` reuses a web build;
`-target-os` and `-target-arch` cross-compile the daemon.

For separate client development:

```sh
cd clients/companion
flutter pub get
flutter run -d chrome
```

Set the daemon address/token in Settings. Local development origins are accepted;
use `--origin https://your-client.example` for an additional trusted browser
origin. HTTPS pages must connect to an HTTPS daemon endpoint. Use HTTPS for
remote native-client connections too.

## Remote access and Telegram

Install and authenticate cloudflared or Tailscale, then use
`openremote tunnel --help`. Tunnels target loopback; Tailscale refuses to replace
existing Serve configuration.

Telegram requires a Bot API token and at least one allowed user ID. The default
chat is optional; select sessions from an allowed chat with `/sessions` and `/use`.

```sh
openremote serve --telegram-token BOT_TOKEN --telegram-user USER_ID --telegram-chat CHAT_ID
```

`TELEGRAM_BOT_TOKEN` can supply the token without a command-line argument.
Commands include `/new <agent> <directory>`, `/use <session>`, `/stop`, and
`/answer <question> <answer>`. `--telegram-topics` enables forum-topic routing.

Stopping a dirty worktree leaves files intact and reports where to review them.
Daemon restart restores transcripts but does not resume agent tasks.

## Validate and release

```sh
go test ./...
go vet ./...
cd clients/companion
flutter analyze
flutter test
flutter build web --release --no-web-resources-cdn
```

These gates use fixtures only. For one real model turn through the real daemon,
install and authenticate an agent CLI, then run the live smoke test from the
repository root (it spends real provider credits and exits 0 only when the
expected token streams back over SSE):

```sh
python scripts/live_e2e.py                # OpenCode, expects OPENCODE_LIVE_OK
python scripts/live_e2e.py pi PI_LIVE_OK  # any registered agent and token
```

Performance targets can be measured locally: `go test -bench . ./internal/core/events ./internal/core/server`
reports event append and end-to-end SSE delivery latency, and `python scripts/measure_idle.py`
samples the idle daemon's memory. Both are noise-sensitive on shared or loaded
machines, so they are not CI gates; see
[implementation status](docs/IMPLEMENTATION_STATUS.md) for recorded figures.

Use `go test -race ./...` with a C toolchain. CI tests Go on Windows/Linux/macOS
and packages six daemon targets. Android distribution signing is described in
[implementation status](docs/IMPLEMENTATION_STATUS.md#reproducible-builds).

[Contributing](CONTRIBUTING.md) · [API](docs/spec/04_PROTOCOL_AND_API_SPEC.md) ·
[Roadmap](docs/spec/06_IMPLEMENTATION_ROADMAP.md) · [MIT license](LICENSE)

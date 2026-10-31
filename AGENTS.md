# OpenRemote

One window for the harnesses on this computer - Claude Code, Codex, grok,
pi, opencode, and agy - run side by side. A Rust daemon wraps the harness
CLIs and serves a Tauri 2 + React 19 console. Local chats never leave the
computer; Cloud mode joins the user's own computers, end to end encrypted
through a moreweb relay that stores nothing.

The workspace-level rules live in `../AGENTS.md` (no secrets echoed, no
spending, no irreversible deletes, commit small and often). This file is
the project's own map.

## Layout

- `src/` - the console (Vite + React 19). One CSS file per view; shared
  chip styles in `composer.css`.
- `src-tauri/` - the desktop shell (frameless, custom titlebar) and the
  app version (`tauri.conf.json`).
- `daemon/` - the Rust workspace (axum HTTP API, one crate per harness,
  `fixture-agent` for e2e). Independently versioned from the app.
  `openremote-cloud` is Cloud mode on a device (moreweb sign-in, the
  device's keys, the registry client, the relay link, the Noise mesh);
  `fake-cloud` is moreweb's contract in memory for the e2e suites and for
  trying Cloud locally (`cargo run -p fake-cloud` prints the env to use).
- `daemon/crates/openremote-daemon/src/pairing.rs` and `pairing_page.rs` -
  Local-mode phone pairing. While a pairing code is live or a phone is
  paired, the daemon opens a second socket: this machine's own LAN address
  (never 0.0.0.0), one of six fixed ports, and a router of four routes
  (counted as the four `route` calls in `pairing.rs`, `routes()`): the page
  the QR code opens, the code exchange, one read-only home route for the
  phone's own token, and a 204 favicon. Nothing else is served there, and
  the socket closes when no code is live and no phone is paired. The
  console side is `src/pairing.js`, `src/qr.js` and
  `src/PairingSection.jsx`; the design note is
  `docs/superpowers/specs/2026-10-26-qr-pairing-design.md`.
- `scripts/` - design instrumentation (fps, latency, clearance), `dev-app.mjs`
  (the dev shell under its own bundle identity), `smoke.mjs` (the one gate that
  reads the running console's real DOM: it drives the dev app over its DevTools
  port and fails on a window that renders nothing, with
  `npm run tauri:dev` running in another terminal), plus `check-tab-acl.mjs`,
  the capability-scope guard CI runs. Run bare it prints
  `tab acl ok: 2 capability file(s), every non-phone one scoped to the main
  webview, every declared command (33) granted` and exits 0, or exits 1 and
  names the capability that widened; `--self-test` checks the check itself,
  and `--root` points it at another tree. Not build tooling; the one build
  script is `sync-daemon.mjs`.
- `Archive/` - measured-and-killed experiments. Consult before
  resurrecting anything; never delete from it.

## Commands

```sh
npm install
npm test               # console tests (node --test)
node scripts/check-tab-acl.mjs   # the tab capability scope (bare; --self-test, --root)
node scripts/smoke.mjs           # the running console's DOM (npm run tauri:dev first)
npm run dev            # browser only (port 5175)
npm run tauri:dev      # desktop shell, under the dev bundle identity
npx tauri dev          # the same build without that identity (see below)
npm run daemon         # build + stage the daemon sidecar
OR_RELEASE=1 npm run daemon   # release profile
npx tauri build        # bundle installers (msi + nsis)
```

The two dev lines differ in one thing, and it matters: the shell names its
single-instance mutex after the bundle identifier, so a dev build that
carries the release identifier cannot start while the installed app is
running. It finds the mutex, hands its arguments to that window and exits,
and the terminal shows a clean exit with no dev window ever appearing.
`npm run tauri:dev` runs `tauri dev --config src-tauri/tauri.dev.conf.json`,
which overrides the identifier for that run only and gives the dev build
its own local data dir; `npx tauri dev` skips the override.

The daemon's own commands (a machine runs `agent` as a systemd user
service, installed by `moreweb.space/openremote/install.sh` from the
website repo):

```sh
openremote-daemon              # the desktop app's sidecar (exits when stdin closes)
openremote-daemon agent        # a machine's service; exits 78 when not enrolled
openremote-daemon enroll <code>   # join the account that minted the code
openremote-daemon status       # 0 when this computer is in Cloud
openremote-daemon unenroll     # leave Cloud (keeps private chats)
# OPENREMOTE_CLOUD_API / OPENREMOTE_AUTH_ISSUER point Cloud at another backend
# (fake-cloud prints them); OPENREMOTE_DEVICE_NAME overrides the hostname.
```

```sh
cd daemon
cargo test --locked            # unit + e2e
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --all --check        # CI runs this and the lint
```

Both gates are green on the committed tree, and CI runs them on every push:
the `daemon` jobs on Windows, macOS and Linux, the MSRV job at 1.85, the
console's suite on all three, and the shell's own clippy and build. They were
not always green: eight daemon files carried uncommitted work for a day, and
the repair landed with the browser merge. A `Diff in` line naming a file you
have not touched is someone else's in-flight edit in a shared working tree, not
a broken commit: check the committed tree before believing it.

`cargo fmt --check` inside `src-tauri` alone is green, because the shell's
own files are formatted.

## The sidecar, and its two copies

The desktop app runs the daemon as a Tauri sidecar (`externalBin:
binaries/openremote-daemon`). `npm run daemon` builds it and stages
`src-tauri/binaries/openremote-daemon-<triple>.exe` - that directory is
gitignored and always regenerated, never committed.

Gotcha: `cargo tauri dev` copies the sidecar into
`src-tauri/target/debug/` only when the app itself rebuilds.
`sync-daemon.mjs` refreshes both copies, but if a daemon fix "isn't
showing up" in the dev app, check that the dev copy's timestamp isn't
stale - and that no running daemon is locking it (EBUSY on copy means
an old process is still alive; find it before re-running).

## Conventions

- Windows first. The frameless window, Segoe MDL2 titlebar glyphs, and
  the dark theme (Inter, Geist Mono, one cool accent and one green) are
  contract items, not preferences.
- `PickerMenu.jsx` is the one dropdown engine. Modals share the
  `dv-modal` family (Esc / backdrop / X to close).
- Harness vocabulary is the harness's own: its model names, its effort
  tiers, its approval words. Never invent OpenRemote substitutes.
- Harness ids (`claude`, `codex`, `grok`, `pi`, `opencode`, `agy`) are
  stable. Display names live in `src/harness-names.js`; brand marks in
  `src/brand-marks.jsx` (verbatim geometry from each owner's own
  published asset - identification use only, no redraws).
- No em dashes anywhere; write around them or use a spaced hyphen.
- Reduced motion is respected twice: `@media (prefers-reduced-motion)`
  and `html[data-reduce-motion]` (the in-app toggle). Every new
  animation covers both.
- Comments explain why, and speak to any reader of the public repo -
  no internal process language, no references to artifacts that no
  longer exist in the tree.

## Modes and views

- Local mode is the default: New chat, Plugins, Automations, and every
  chat that runs on this computer. Cloud lists every synced chat and has
  Machines; until this computer is signed in, Cloud's main area is its
  sign-in screen (its sky in `cloudmode.css` is deliberately distinct
  from onboarding's violet). The mode is remembered.
- `src/cloud.js` holds the pure rules (which chats a mode lists, what can
  be done with a chat on another device, the machine picker); it has
  tests, keep them that way.
- View state: `STATIC_VIEWS` in App.jsx is the route list. A stale hash
  or stored history falls back to New chat - new views must be added
  there or they never resolve.
- Mobile is both halves of the phone story, and they do not depend on each
  other: a phone pairs by QR code over the local network with no account
  (the socket above, `src/PairingSection.jsx`), and a phone signed in to
  the account lists the synced chats and can drive a machine. Pairing
  works while Cloud says the user is signed out.

## Cloud's trust model, for anyone touching it

- The console only ever talks to its own daemon on 127.0.0.1. That daemon
  reaches the user's other devices through the relay (`/devices/{id}/...`
  is another device's API, carried over the mesh).
- What a peer may ask is the allowlist in `http.rs` (`PEER_ROUTES`), in
  three tiers: copying synced chats (any device of the account), driving
  the computer (only when this computer is a machine; the caller's own
  kind is not checked, so a phone may drive a machine), and acting on one
  chat (a machine, and the chat is synced and runs here). Private chats
  never leave the computer. Add to that list with care: everything on it
  is reachable by every device of the account.
- What a peer can therefore do is more than drive: it can create a chat
  here with an approval-bypassing permission word, change a stopped
  chat's word so the next resume runs with it, answer the approval
  prompts through `/decisions/{id}/answer`, delete a synced chat
  through a `sync.tombstone` note, which is not a route and so sits
  outside `PEER_ROUTES` (`on_note`, `sync.rs`), and rewrite this
  computer's browser settings and bookmark tree through a `sync.browser`
  note, deletions included, with no confirm and no record of which device
  did it. The approval gate guards against a harness, not against a
  device; it is not a promise that a peer cannot answer.
  `docs/cloud-trust-model.md` is the route-by-route table, with what does
  protect the user instead.
- Secrets (the daemon token, `state.json`, `cloud/`) are owner-only files.

## Releases

- Version lives in `package.json`, `src-tauri/tauri.conf.json`,
  `src-tauri/Cargo.toml` (installer filenames embed it). The daemon
  workspace carries its own version. `bundle.android.versionCode` in
  `tauri.conf.json` must strictly increase with every release -
  Android refuses to install an update whose versionCode does not
  exceed the installed build's.
- `.github/workflows/release.yml` builds on `v*` tag pushes: a preflight
  refuses a tag whose commit is not on the default branch's history, and
  `npm test` runs before the build. Then the sidecar is staged, `npx tauri
  build`, `gh release create` with the notes from `release-notes/<tag>.md`.
  It also runs nightly (03:17 UTC)
  to publish the rolling `nightly` prerelease from the default branch,
  and a manual run stays build-only unless its channel input says
  `nightly`. Every published release carries the six version-free asset
  names (`OpenRemote-Setup-x64.exe`, `-Setup-x64.msi`, `-x86_64.AppImage`,
  `-amd64.deb`, `-universal.dmg`, `-universal.apk`) so
  `releases/latest/download/<name>` is the stable channel and
  `releases/download/nightly/<name>` the nightly one; the Android APK is
  the one that also ships under its versioned name beside them. `docs/release-channels.md`
  has the details and the nightly's prerelease rule. The Linux job also uploads the
  machine installer's download: `openremote-daemon-linux-x86_64` plus
  `SHA256SUMS`, cross-built with zig pinned to glibc 2.28 (a runner
  build links a glibc old servers don't have; install.sh verifies the
  checksum and refuses a mismatch). Push main before pushing the
  tag.
- Installers are unsigned by decision; the release notes say so
  (SmartScreen, More info > Run anyway).
- `npm/` is a second front door: a small launcher published to the registry as
  `@morewebs/openremote`. It holds no build, it reads `releases/latest` at run
  time, fetches the portable Windows pair the Windows job stages beside the six
  names above, verifies both against `SHA256SUMS-portable-windows-x64`, keeps them
  under the user's local app data and starts the console. So a launcher version
  does not pin a release, and publishing the launcher is a manual `npm publish`
  from `npm/` that no workflow performs - a release without it is still a release,
  on the installer channel.
- License is AGPL-3.0. Harness names and marks belong to their
  owners; OpenRemote is not affiliated with them.

## When the build breaks strangely

- `LNK1318` on a PDB usually means the C: drive is full, not a lock -
  check `df -h /c` before chasing processes.
- Flaky-looking daemon e2e failures under heavy parallel load: rerun
  the suite once before diagnosing; the fixture-agent e2e is timing
  sensitive on a busy machine.
- `failed to run custom build command for openremote-ui-desktop` with a
  `PermissionDenied` (`Os { code: 5 }`) panic inside `tauri-build` is not a
  broken build: it is the dev copy of the sidecar still held open by a running
  daemon (`target/debug/openremote-daemon.exe`), which the build script rewrites
  whenever the app version changes - so a version bump taken while the dev app is
  up is when it shows. Quit the dev app, or set `CARGO_TARGET_DIR` elsewhere, and
  `cargo test` and clippy pass.

## Checking this file

Four commands check what this page claims, and a reader can run all of them
from the repo root: `npm test` (prints a count; 1108 the last time this line
was checked, and the run is the truth rather than this number), `node
scripts/check-tab-acl.mjs` (says `tab acl ok: 2 capability file(s), every
non-phone one scoped to the main webview, every declared command (33)
granted`), `cd daemon && cargo fmt --all --check` and
`cd daemon && cargo clippy --all-targets --locked -- -D warnings` (both print
nothing and exit 0 on the committed tree). The route and tier claims in "Cloud's
trust model" are counted from `PEER_ROUTES` in
`daemon/crates/openremote-daemon/src/http.rs`, the peer paths are driven in
`daemon/crates/openremote-daemon/tests/e2e_cloud_mesh.rs`, and
`docs/cloud-trust-model.md` keeps the rows it could not exercise under
"Unverified on this page", a section before the closing "If you are
changing this".

# OpenRemote

One window for the harnesses on this computer - run Claude Code, Codex,
and the other harnesses side by side. A Rust daemon on this machine; a
desktop console (Tauri 2 + React 19) for everything else.

## Install

Grab the latest installer from
[Releases](https://github.com/morewebs/OpenRemote/releases) (Windows 10+,
MSI or the NSIS setup exe). The installers are unsigned - SmartScreen
will ask; choose More info > Run anyway.

Or, from a terminal with Node 18.17 or newer:

```sh
npm install -g @morewebs/openremote
openremote
```

That installs a launcher, not the app. The first `openremote` downloads the
portable build from the current release (about 28MB, into
`%LOCALAPPDATA%\moreweb\openremote\<release>\`), checks it against the
release's own checksums, and starts it; later runs use that copy and need no
network, and `openremote --update` moves to a newer release. It is the same
binary the installer carries, under the same identity, so whichever way you
install it you get the same chats, settings and paired phone. Windows x64
only for now - `openremote --help` says so, and names the installer for
macOS and Linux. The package itself is [npm/](npm); `docs/release-channels.md`
says how a release publishes it.

## Layout

- `src/` + `src-tauri/` - the console (Vite + React 19), frameless Tauri 2 shell
- `daemon/` - the Rust workspace that wraps the harness CLIs and serves the
  console

## Develop

Console (port 5175):

```sh
npm install
npm run dev            # browser only
npx tauri dev          # desktop shell (Windows tested)
```

The desktop shell runs the daemon as a sidecar; stage its binary first
(and after daemon changes):

```sh
npm run daemon         # debug daemon; OR_RELEASE=1 for release
```

To run the daemon yourself (the console's first-run steps can connect to
it by address):

```sh
cd daemon && cargo run
```

It prints `READY 127.0.0.1:<port>` on stdout; all logs go to stderr. Its
data dir is `~/.openremote` (override with `OPENREMOTE_DATA_DIR`), and the
bearer token lives in `~/.openremote/token`.

Tests:

```sh
npm test               # console tests: 279 when this line was last checked
node scripts/check-tab-acl.mjs   # what the desktop webview's capabilities grant
cd daemon && cargo test --locked   # unit + e2e (drives the fixture agent)
```

The first two run from the repo root and take no arguments. The console
suite covers the rules behind each view and reads a few source files for
the wiring only a browser could show; the daemon suite drives a real
sidecar. Two further gates, `cd daemon && cargo fmt --all --check` and
`cd daemon && cargo clippy --all-targets --locked -- -D warnings`, are red
in this tree on eight unformatted files and three lints, none of them in
the shell, so read their output before blaming your own change. AGENTS.md
lists both.

## Cloud mode

Local chats never leave the computer and need no account. Cloud mode joins
your own computers - your laptops, a home server, a VPS - through your
moreweb account: chats run on whichever one you pick and are copied to all
of them, end to end encrypted. moreweb's relay only ever forwards
ciphertext and stores no chats.

Add a Linux machine from Cloud > Machines > Add a machine: it gives you a
one-time command that installs OpenRemote as a background service and
joins it to your Cloud. To try Cloud mode without moreweb, run the
stand-in and point the daemon at it:

```sh
cd daemon && cargo run -p fake-cloud   # prints the OPENREMOTE_* exports
```

## License

OpenRemote is licensed AGPL-3.0-only - see [LICENSE](LICENSE).

## Conventions

- Windows first: frameless window with a custom titlebar (Segoe MDL2 glyphs).
  Dark theme, Inter (+ Geist Mono for technical content), one cool accent and
  one green.
- `PickerMenu.jsx` is the one dropdown engine everywhere; modals all share the
  `dv-modal` family (Esc / backdrop / X to close).
- Harness names and marks belong to their owners; OpenRemote is not
  affiliated with them.
- Everything above is checked by the commands under Tests: if a claim here
  and a command's output disagree, the command is right.

# OpenRemote

One window for the harnesses on this computer - run Claude Code, Codex,
and the other harnesses side by side. A Rust daemon on this machine; a
desktop console (Tauri 2 + React 19) for everything else.

## Install

Grab the latest installer from
[Releases](https://github.com/morewebs/OpenRemote/releases) (Windows 10+,
MSI or the NSIS setup exe). The installers are unsigned - SmartScreen
will ask; choose More info > Run anyway.

## Layout

- `src/` + `src-tauri/` - the console (Vite + React 19), frameless Tauri 2 shell
- `daemon/` - the Rust workspace that wraps the harness CLIs and serves the
  console

## Develop

Console (port 5173):

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
npm test               # console reducer tests
cd daemon && cargo test --locked   # unit + e2e (drives the fixture agent)
```

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

## Conventions

- Windows first: frameless window with a custom titlebar (Segoe MDL2 glyphs).
  Dark theme, Inter (+ Geist Mono for technical content), amber/green accents;
  the design contract lives in [DESIGN.md](DESIGN.md).
- `PickerMenu.jsx` is the one dropdown engine everywhere; modals all share the
  `dv-modal` family (Esc / backdrop / X to close).
- Harness names and marks belong to their owners; OpenRemote is not
  affiliated with them.

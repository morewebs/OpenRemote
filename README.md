# OpenRemote

One window for the harnesses on this computer - run Claude Code, Codex,
and the other harnesses side by side. A Rust daemon on this machine; a
desktop console (Tauri 2 + React 19) for everything else.

Status: rebuilding from the owner's UI prototype as the design baseline
(second true-zero, 2026-10-01). The design system is contracted in
[DESIGN.md](DESIGN.md) - the console transcribes it, never improvises past it.

## Layout

- `src/` + `src-tauri/` - the console (Vite + React 19), frameless Tauri 2 shell
- `daemon/` - the Rust workspace that wraps the harness CLIs and serves the
  console (rebuilding; lands next in the slice)

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

## Conventions

- Windows target first: frameless window with a custom titlebar (Segoe MDL2
  glyphs). Dark theme, Inter (+ Geist Mono for technical content), amber/green
  accents - the contract lives in [DESIGN.md](DESIGN.md).
- `PickerMenu.jsx` is the one dropdown engine everywhere; modals all share the
  `dv-modal` family (Esc / backdrop / X to close).
- `scripts/` holds the design's instrumentation (fps, scroll latency, modal
  clearance); `Archive/` holds measured-and-killed experiments - consult
  before resurrecting anything.

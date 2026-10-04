# OpenRemote

One window for the harnesses on this computer - Claude Code, Codex, grok,
pi, opencode, and agy - run side by side. A Rust daemon wraps the harness
CLIs and serves a Tauri 2 + React 19 console. Everything stays on the
machine.

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
- `scripts/` - design instrumentation (fps, latency, clearance). Not
  build tooling; the one build script is `sync-daemon.mjs`.
- `DESIGN.md` - the design contract. The console transcribes it, never
  improvises past it.
- `Archive/` - measured-and-killed experiments. Consult before
  resurrecting anything; never delete from it.

## Commands

```sh
npm install
npm test               # console tests (node --test, 30)
npm run dev            # browser only (port 5173)
npx tauri dev          # desktop shell
npm run daemon         # build + stage the daemon sidecar
OR_RELEASE=1 npm run daemon   # release profile
npx tauri build        # bundle installers (msi + nsis)
```

```sh
cd daemon
cargo test --locked            # unit + e2e
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --all --check        # CI enforces both
```

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
  the dark theme (Inter, Geist Mono, amber/green accents) are contract
  items, not preferences.
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

- Local mode is the default: New chat, Plugins, Automations, the
  project list. Machines is a cloud-mode view; its sidebar tile
  renders only in cloud mode (grid-rows reveal, reduced-motion aware).
- Cloud mode swaps the whole main area for its coming-soon state;
  session history survives underneath. Its sky in `cloudmode.css` is
  deliberately distinct from onboarding's violet.
- View state: `STATIC_VIEWS` in App.jsx is the route list. A stale hash
  or stored history falls back to New chat - new views must be added
  there or they never resolve.

## Releases

- Version lives in `package.json`, `src-tauri/tauri.conf.json`,
  `src-tauri/Cargo.toml` (installer filenames embed it). The daemon
  workspace carries its own version.
- `.github/workflows/release.yml` builds on `v*` tag pushes:
  stage the sidecar, `npx tauri build`, `gh release create` with the
  notes from `release-notes/<tag>.md`. Push main before pushing the
  tag.
- Installers are unsigned by decision; the release notes say so
  (SmartScreen, More info > Run anyway).
- License is AGPL-3.0. Harness names and marks belong to their
  owners; OpenRemote is not affiliated with them.

## When the build breaks strangely

- `LNK1318` on a PDB usually means the C: drive is full, not a lock -
  check `df -h /c` before chasing processes.
- Flaky-looking daemon e2e failures under heavy parallel load: rerun
  the suite once before diagnosing; the fixture-agent e2e is timing
  sensitive on a busy machine.

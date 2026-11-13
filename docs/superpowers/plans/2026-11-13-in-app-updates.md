# In-app updates: channels, feeds, and the badge (v1)

Status: being built on `main`. Nothing is pushed, tagged or published. Delta patches are deferred (see below).

## Decided

- **Two apps, one codebase.** Stable and nightly are separately installed apps
  with their own identifier, product name, data folder, update feed and updater
  key. A nightly install can never be offered a stable build, or the reverse.
- **v1 covers Windows and Linux.** macOS and Android keep shipping as they do
  now, outside the updater.
- **Nightly** builds on every push to `main` whose CI passes. Version
  `<base>-nightly.<UTC YYYYMMDDHHmm>`, where `<base>` is the `version` in
  `tauri.conf.json` with its patch number raised by one (0.5.0 gives
  `0.5.1-nightly.202611130215`). Windows (NSIS only, because MSI cannot carry a
  pre-release version) and Linux (AppImage) only.
- **Stable** is cut by a `vX.Y.Z` tag, as today. The release refuses a tag whose
  version does not match `tauri.conf.json`.
- **Hosting: GitHub Releases for both channels** (the repo is public, so clients
  need no auth). Stable feed: `releases/latest/download/latest.json`. Nightly
  feed: `releases/download/nightly/latest.json`, on a rolling `nightly` release
  that is re-pointed on every build. Every nightly is also a dated release
  `nightly-<stamp>`; the newest 20 are kept and older ones are deleted with their
  tags.
- **Badge UI.** The info button in the top bar becomes a blue "Update available"
  badge when a newer version exists. Clicking it opens a confirmation. If chats
  are running, the badge and its confirm button turn red and the confirmation
  warns how many chats will stop.
- **Checks.** On launch (30 s after start, so it never competes with startup),
  then every 2 hours while the app runs. Automatic checks can still be turned
  off in the console.
- **Windows** downloads the new installer in the background as soon as a check
  finds it, keeps the bytes in memory, verifies the Tauri signature, and installs
  from memory when the user confirms. The daemon is stopped first, as today.
- **Linux is link-only.** The badge opens the release page. The app never
  replaces an AppImage or a deb.
- **The feed format is Tauri's `latest.json`**, plus one extra top-level field,
  `page`: the release page the badge opens. Tauri's plugin ignores unknown
  fields, so the plugin still reads the file.

## Deferred: delta patches

The plan was a binary patch between consecutive builds. Measured on a 23 MB
Windows daemon binary with a 2 KiB change: the patch carries 2 KiB when the
file is raw, but 66% of the file when it is LZMA-compressed. Installers and
AppImages are compressed, so patching the installer would save little. The
patch path is not built. Choosing how deltas should work is a decision for the
user before that path is written. The options are: patch uncompressed
application files in place, ship uncompressed installers for updates only, or
keep full installers.

## Signing

Each channel has its own minisign key, made with `npx tauri signer generate`.
The public half goes in the channel's config (`plugins.updater.pubkey`), the
private half and its password go in GitHub secrets. A build with an empty
public key is not switched on and never asks the network. The runbook is
`docs/releasing-updates.md`.

Secret names:

- stable: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (as today)
- nightly: `NIGHTLY_TAURI_SIGNING_PRIVATE_KEY`, `NIGHTLY_TAURI_SIGNING_PRIVATE_KEY_PASSWORD`

## Identity per channel

| | stable | nightly |
|---|---|---|
| `productName` | `OpenRemote` | `OpenRemote Nightly` |
| `identifier` | `space.moreweb.openremote` | `space.moreweb.openremote.nightly` |
| feed | `releases/latest/download/latest.json` | `releases/download/nightly/latest.json` |
| installer asset | `OpenRemote-Setup-x64.exe` | `OpenRemote-Nightly-Setup-x64.exe` |
| AppImage asset | `OpenRemote-x86_64.AppImage` | `OpenRemote-Nightly-x86_64.AppImage` |
| release page | `releases/tag/<vX.Y.Z>` | `releases/tag/nightly` |

The nightly identity lives in `src-tauri/tauri.nightly.conf.json`, a Tauri
config overlay. The nightly workflow writes the version into a generated overlay
and builds with both files.

## Workstreams and file ownership

- **Release and CI** (`.github/workflows/`, `scripts/`, `src-tauri/tauri.nightly.conf.json`,
  `docs/releasing-updates.md`, `docs/release-channels.md`).
- **Shell** (`src-tauri/src/updates.rs`). Nothing else in `src-tauri/` changes.
- **Console** (`src/updates.js`, `src/updates.test.js`, the top-bar info button and
  its confirmation, and their CSS).

No workstream commits. The coordinator commits, file by file, and never stages
`daemon/crates/openremote-cloud/src/mesh.rs`, which is someone else's edit in the
shared working tree.

## Acceptance

- `npm test` passes, with new tests for the check interval, the channel-aware
  version compare, the badge states, and the red-when-chats-running rule.
- `cargo test` passes for the shell, including the install-slot and daemon-order
  tests that already exist.
- `node scripts/updater-manifest.mjs` tests pass, covering the stable and nightly
  feeds and the version rules.
- The nightly and release workflows parse (YAML), and every `${{ }}` reference
  points at a name that is defined.
- Nothing is pushed, tagged, or published. The first real proof is one nightly run
  after the user adds the keys and secrets.

## User actions (not done by this work)

1. Generate the two signing keys with `npx tauri signer generate`, keep them out of the repo.
2. Put the stable public key in `tauri.conf.json` and the nightly public key in
   `tauri.nightly.conf.json` (the nightly overlay file records where).
3. Add the four secrets listed above.
4. Run the nightly workflow once and check the feed and the installed app.

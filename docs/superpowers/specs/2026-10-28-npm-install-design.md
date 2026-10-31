# `npm install -g` as an install channel for OpenRemote

2026-10-28. Session note for the npm install channel.

## Amendments as built

Five things differ from this design, each for a reason found while building
it. The code and `docs/release-channels.md` are the truth; this section
records why they moved.

1. **The channel is the npm registry, not GitHub shorthand.** npm's global
   install of a git dependency of this repo leaves a package directory with
   no launcher in it (npm 10 and 11 on Windows, reproduced five times, while
   an ordinary repo installs correctly). A published tarball install works,
   so `@morewebs/openremote` is published from `npm/` and that is the channel.
2. **The launcher is its own package** (`npm/package.json`), not a `bin` on
   the app's manifest. The repo root stays `private: true`.
3. **The portable pair stays outside the six required release names.** A
   missing pair warns in the run instead of failing it, because a release
   must never be held back by the second door.
4. **A checksum record lives in each release directory**
   (`checksums.json`) as well as at the cache root, so losing `current.json`
   does not discard a cache that still verifies.
5. **The app's runtime dependencies stay in `devDependencies`.** They were
   moved for the abandoned git-install path and stay because nothing reads
   `node_modules` at run time; the published launcher is a 5kB tarball
   either way.

## Intent

A person who lives in a terminal should be able to install the desktop app
without downloading and running an installer:

```sh
npm install -g morewebs/openremote
openremote
```

`openremote` opens the same frameless console the installer's Start menu
entry opens, and the daemon it needs is already compiled. Nothing about the
app changes. This is a new front door to an existing product, not a second
product.

Why: installs are the scoreboard (`Documentation/growth.md`). The installer
is the only way in today, and it asks a developer to leave the terminal,
clear SmartScreen, and click through a wizard. npm is where that person
already is, and it puts `openremote` on the `PATH` next to the harness CLIs
the app drives.

## What exists

- The console is a Tauri 2 shell (`src-tauri/`, exe `openremote-ui-desktop.exe`)
  that runs `openremote-daemon` as a sidecar from its own directory. The
  console is inert without that pair.
- `package.json` at the repo root is the app's build manifest: `private`,
  Vite/Tauri scripts, React deps. It has no `bin`, so
  `npm install -g morewebs/openremote` (npm's GitHub shorthand) has nothing
  to run.
- `releases/latest` is the stable channel, and every release carries
  version-free asset names on purpose (`.github/workflows/release.yml`),
  so `releases/latest/download/<name>` never moves with a version bump.
  It currently has six installers and no portable pair.
- Verified on 2026-10-28: the pair from the published `v0.4.1` msi
  (`msiexec /a`) launches from an empty directory with no installer, no
  registry entries, and no neighbouring DLL, spawning its own daemon.
  `releases/latest` redirects to `releases/tag/v0.4.1`, so an installer can
  learn which release it just fetched.

## Decisions taken with the human

1. **GitHub shorthand, not the npm registry.** `npm install -g
   morewebs/openremote` clones the repo. No npmjs.com publication, no npm
   account, no 2FA tonight. A registry package can follow; it would share
   everything below.
2. **Windows first.** The launcher covers win32-x64 because it can be
   launched and verified here. macOS and Linux resolve to a clear message
   that names the `.dmg`/`.AppImage`, and stay unverified until CI can run
   them.
3. **Download on first run.** The repo stays source-only. `openremote`
   fetches the pair once into a per-release cache and spawns from there.

## Approaches considered

- **A. Vendor the pair in the repo** (`npm/win32-x64/`). Works offline, no
  release dependency, pinned to the commit you install. Rejected: ~15MB in
  git in every clone of a public repo, forever, and every release has to
  refresh the vendored copy (a CI job committing binaries).
- **B. Download from the release on first run.** Chosen. Keeps the release
  channel as the one host for binaries and the repo as source.
- **C. Browser mode, no exe.** `openremote` runs the daemon and opens the
  console in a browser. Rejected: the frameless titlebar, native dialogs,
  autostart and the sidecar are console contract items, and the console
  calls Tauri APIs the browser does not have.

## What the change touches

**`package.json`** gains `bin: { openremote: "npm/bin/openremote.mjs" }`,
`files: ["npm"]` and `engines.node`. The app's runtime dependencies
(react, react-dom, katex, phosphor, fontsource, `@tauri-apps/api`,
simple-icons) move to `devDependencies`: Vite bundles them into `dist/`, so
nothing at run time reads `node_modules`, and a *global* git install would
otherwise download ~45MB of React to launch an exe. CI's `npm ci` installs
devDependencies, so the build is unaffected.

**`npm/bin/openremote.mjs`** is the whole mechanism, dependency-free, run by
Node:

- win32-x64 only tonight. Any other platform prints the installer URL for
  that platform and exits 1.
- Resolves the release to install: the cached one if present, otherwise
  `releases/latest`, whose redirect names the tag.
- Downloads two version-free assets from that tag and asks for nothing else:
  `OpenRemote-windows-x64.exe` and `openremote-daemon-windows-x64.exe`,
  plus `SHA256SUMS-portable-windows-x64`. Each is verified before anything
  is spawned; a mismatch removes the partial download and fails loudly.
- Caches under `%LOCALAPPDATA%\moreweb\openremote\<tag>\` as `OpenRemote.exe`
  and `openremote-daemon.exe`. The sidecar's filename is the contract the
  Tauri shell resolves, so it is restored on write, not merely preserved.
  A per-tag directory means an upgrade never has to overwrite a running exe.
- Spawns detached with no inherited stdio, prints where it is running from,
  and exits. The window outlives the terminal, which is what a terminal
  user expects from `code .` or `zed`.
- Second and later runs read the cache and touch the network zero times.
  The app has never looked for updates on its own; `--update` opts in,
  `--version` and `--where` report, `--help` explains.

**`.github/workflows/release.yml`**: the Windows job stages the pair it just
built under the version-free names beside the installers, and the publish job
uploads them. They join the completeness list, with a comment recording that
`0.4.1` is the first release that carries them.

**The `v0.4.1` release** is backfilled by extracting the pair from its own
published msi, so the bytes are the ones the installer ships rather than a
local rebuild of a different commit.

**`README.md`** and `docs/` gain the two-line install. `AGENTS.md`'s
Commands block gains the launcher's flags.

## Non-goals

- No publication to the npm registry, no `@morewebs` scope, no npm token.
- No change to the installers, the update story, or the app itself.
- No macOS or Linux binaries tonight.
- Not a service installer: `openremote` runs the desktop app, not
  `openremote-daemon agent`.

## Anti-goals

- Silently downloading 28MB without saying so, or spawning a binary whose
  checksum was never checked.
- Breaking the repo's own `npm install`, `npm test`, or the release
  pipeline for contributors who never touch the launcher.
- A second install path that drifts from the release channel: if the
  launcher needs a different binary layout than the release ships, the
  launcher is wrong.

## Verification

1. `npm test` and `node scripts/check-tab-acl.mjs` on the branch.
2. `npm pack --dry-run` shows `npm/bin/openremote.mjs` and no binaries.
3. From a scratch prefix: `npm install -g github:morewebs/OpenRemote#npm/github-install`,
   then run the installed `openremote`, watch the window open and the cache
   populate, and confirm the daemon it spawned is the cached copy.
4. Second run: no network, window opens, no re-download.
5. Corrupt a cached file, re-run, confirm the checksum refusal path.
6. The real command, on main, after the merge.

## Left to the builder

Retry and progress wording for the download, how old cached tags are pruned,
whether `--update` also accepts a channel name, and the exact phrasing of the
non-Windows message.

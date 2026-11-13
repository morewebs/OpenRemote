# The release channels

Two channels, two workflows. Stable is `.github/workflows/release.yml`. Nightly
is `.github/workflows/nightly.yml`. Their feeds, keys and in-app update behaviour
are in `docs/releasing-updates.md`.

## Stable

A `v*` tag. Its release is the newest release in the repo, so these URLs
always point at the latest stable build:

```
https://github.com/morewebs/OpenRemote/releases/latest/download/OpenRemote-Setup-x64.exe
https://github.com/morewebs/OpenRemote/releases/latest/download/OpenRemote-Setup-x64.msi
https://github.com/morewebs/OpenRemote/releases/latest/download/OpenRemote-x86_64.AppImage
https://github.com/morewebs/OpenRemote/releases/latest/download/OpenRemote-amd64.deb
https://github.com/morewebs/OpenRemote/releases/latest/download/OpenRemote-universal.dmg
https://github.com/morewebs/OpenRemote/releases/latest/download/OpenRemote-universal.apk
https://github.com/morewebs/OpenRemote/releases/latest/download/openremote-daemon-linux-x86_64
https://github.com/morewebs/OpenRemote/releases/latest/download/SHA256SUMS
https://github.com/morewebs/OpenRemote/releases/latest/download/OpenRemote-windows-x64.exe
https://github.com/morewebs/OpenRemote/releases/latest/download/openremote-daemon-windows-x64.exe
https://github.com/morewebs/OpenRemote/releases/latest/download/SHA256SUMS-portable-windows-x64
```

Every published release carries both names for each bundle: the versioned
one Tauri produces (`OpenRemote_0.3.2_x64-setup.exe`) and the version-free
one above. A version bump never edits a download link anywhere.

The version-free names start with the first tag after this landed - v0.3.2
predates them, so its release page has only the versioned assets.

## The npm channel

`npm install -g @morewebs/openremote` installs the launcher in `npm/`, a
package of three files (its manifest, this description, and
`npm/bin/openremote.mjs`) published separately from the app.

The launcher carries no binary. On its first run it follows
`releases/latest` to the current tag, downloads the three portable assets
above, and verifies both `.exe` files against
`SHA256SUMS-portable-windows-x64` before anything is spawned. It then keeps
them as `OpenRemote.exe` and `openremote-daemon.exe` under
`%LOCALAPPDATA%\moreweb\openremote\<tag>\` - the second name is the contract
the Tauri shell resolves beside itself - and records the checksums it
verified in `current.json` there. A later run checks against that record, so
it makes no request at all, and a copy that no longer matches is fetched
again rather than started.

The three assets are the same binaries the installer carries, staged from
the same build. They are deliberately outside the six required names the
publish job enforces: the npm channel is a second door to the app, so a
release that could not produce them still publishes its installers and says
so in the run.

**A release with no portable pair is invisible to npm.** Every download
failure names the URL that answered badly and then the installer to use
instead, and nothing is spawned: the launcher fails closed rather than
starting something half-right.

### Publishing the launcher with a release

The launcher names a release, it is not part of one, so it is published by
hand when the app's version moves:

```sh
cd npm
npm publish --access public
```

It has to be a terminal you are sitting at, not CI: the account's two-factor
requirement is satisfied by approving the request in a browser, and npm is
retiring tokens that bypass it for direct publishing. A headless run gets
`E403` no matter what code it offers.

`npm/package.json` carries the app's `version` and must move with it: the
repo's tests assert the two agree, so a version bump that forgets the npm
manifest fails `npm test` rather than shipping a launcher that claims to be
a release it is not.

Installing from GitHub instead - `npm install -g morewebs/openremote` -
is not supported: npm's global install of a git dependency of this repo
produces a package directory with no launcher in it (verified on npm 10 and
11 on Windows), while an install from a published tarball works. That is
why the channel is the registry.

## A tag must match the config and be on the default branch

The release job's preflight refuses a `v*` tag unless it is `v` followed by the
`version` in `src-tauri/tauri.conf.json`, exactly. It also refuses a tag whose
commit is not on the default branch's history (GitHub's compare API must report
`identical` or `behind`). A tag pushed at a local commit, or at work on a branch
that was never merged, would otherwise publish installers built from code the
project never accepted, and move `releases/latest` and the site's download
buttons for every user. Merge first, then tag the commit that is on the default
branch.

## Nightly

Built by `.github/workflows/nightly.yml` after CI passes on a push to `main`.
Windows (an NSIS installer) and Linux (an AppImage) only; there is no MSI, deb,
macOS or APK nightly.

```
https://github.com/morewebs/OpenRemote/releases/download/nightly/OpenRemote-Nightly-Setup-x64.exe
https://github.com/morewebs/OpenRemote/releases/download/nightly/OpenRemote-Nightly-x86_64.AppImage
```

- The version is `<base>-nightly.<stamp>`, described in `docs/releasing-updates.md`.
  The app reports that version, so each build names itself.
- One rolling release at the tag `nightly`. Its tag moves to the built commit and
  its installers and `latest.json` are replaced, so its URLs never change while
  the binaries follow `main`.
- Every build is also a dated release, `nightly-<stamp>`. The newest 20 are kept,
  and older ones are deleted with their tags.
- **Always a prerelease.** That keeps `releases/latest`, and with it the site's
  download buttons, on the stable channel.
- A failed build publishes nothing, so the last nightly stays up.

### Firing it by hand

Actions > Nightly > Run workflow. It builds `main` as it stands and publishes,
the same as a run after CI. Nightly runs are never scheduled; they follow CI.

## Android versionCode

Each stable release's `bundle.android.versionCode` must strictly exceed the
previous tag's, or Android refuses the update install. The release preflight
checks it for stable tags. The nightly builds no APK.

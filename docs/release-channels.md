# The release channels

Two channels, one pipeline (`.github/workflows/release.yml`).

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

## An installed app updates on the user's click

The desktop app looks once a day for `latest.json` in the newest stable
release and offers a newer version, which installs only when the user clicks
Update and restart (`docs/releasing-updates.md`). Updates are signed, and the
switch-on is a one-time step: until the signing key exists, the app says
updates aren't switched on, and moving to a newer release means downloading
the installer from the URLs above and running it over the copy already there.
The nightly is never offered in-app: the app reads `releases/latest` only.

## A tag must be on the default branch

The release job's preflight refuses a `v*` tag whose commit is not on the
default branch's history (GitHub's compare API must report `identical` or
`behind`). A tag pushed at a local commit, or at work on a branch that was
never merged, would otherwise publish installers built from code the
project never accepted, and move `releases/latest` and the site's download
buttons for every user. Merge first, then tag the commit that is on the
default branch.

## Nightly

The scheduled run, 03:17 UTC daily:

```
https://github.com/morewebs/OpenRemote/releases/download/nightly/OpenRemote-Setup-x64.exe
```

(the same names as the stable column, with `releases/download/nightly`
instead of `releases/latest/download`)

- Builds whatever is on the default branch, with no tag of its own. The
  release body names the commit, the build time, and the commits since the
  previous nightly.
- One rolling release at the tag `nightly`, deleted and recreated each
  night, so the URLs never change while the binaries follow `main`.
- **Always a prerelease.** That is what keeps `releases/latest` - and with
  it the site's download buttons - on the stable channel. Never publish a
  nightly as a full release.
- A failed night leaves the channel empty until the next run (last night's
  release is deleted before tonight's is built, rather than serving a
  half-updated build). Re-running the failed scheduled run publishes it.
- Nightlies are not version-stamped: the app reports the stable version,
  so the release page's commit is the build's identity. Stamping a
  per-night version would have to survive MSI's numeric-only prerelease
  and macOS's numeric `CFBundleVersion`, which needs a real build on both
  to prove; until then the commit is the label.
- The APK keeps the stable `applicationId` and `versionCode`, so a nightly
  installs over a stable one and a stable one installs over a nightly
  (same signature, same code) - no uninstall, no wiped data.

### Firing it by hand

Actions > Release > Run workflow, channel `nightly`. With `build-only` the
run builds everything and publishes nothing, exactly as before.

Note that GitHub disables a scheduled workflow after 60 days of repository
inactivity; re-enable it in the Actions tab, then run it once by hand.

## Android versionCode

Unchanged rule, checked in the preflight for stable tags: each release's
`bundle.android.versionCode` must strictly exceed the previous tag's, or
Android refuses the update install. The nightly is exempt because it is
not a stable tag - and because it reuses the stable code on purpose.

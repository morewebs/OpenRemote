# In-app updates

Two apps come from this codebase: OpenRemote (stable) and OpenRemote Nightly.
They are separate installs with their own product name, identifier, data folder,
feed and updater key, so a nightly install is never offered a stable build, or
the reverse.

Each app checks its own feed thirty seconds after it starts, then every two
hours while it runs. Automatic checks can be turned off in the console. On
Windows the app fetches a newer installer in the background as soon as a check
finds it, keeps the bytes in memory, and verifies each file it takes against its
signature. It installs from memory when the user confirms, and the daemon is
stopped first. An update whose signature does not verify is refused. On Linux
the badge only opens the release page; the app never replaces an AppImage.

## Smaller Windows updates

Besides its own installer, a Windows release ships up to three signed files for
the app to choose from, and the app takes the smallest route it can use:

- **a patch** from the previous release's uncompressed installer
  (`OpenRemote-Setup-x64.patch.zst`, or the nightly's own name). It only exists
  when the previous release stored its installer, and the app only uses it when
  it kept that exact installer;
- **the whole new installer, packed** (`OpenRemote-Setup-x64.update.zst`, about
  11 MB);
- **the release's own installer**, the LZMA one a fresh download gets (about
  10 MB). An app with nothing else still updates from it.

The patch and the packed installer come from an uncompressed build of the same
app (NSIS `compression: none`, 43.6 MB). An LZMA installer changes almost
everywhere when the app changes, so two of them cannot be patched small; the
uncompressed one changes where the app did. The release builds it second, from
the same source, and `scripts/make-update-delta.sh` makes the files and signs
them. A build without the signing key makes none of them.

After an update lands, the app keeps its uncompressed installer packed at zstd
level 3 (about 14 MB) in its data folder, under `updates/`, as the base for the
next patch. A fresh install has no base, so its first update is the packed
installer; every later update is a patch. Only the installed version's base is
kept. The level is a trade: level 19 gives 11.2 MB but takes about 13 seconds,
on the install path.

Measured on this machine: a 2 KiB change to the uncompressed installer gives a
6 KB patch. Making a patch takes about 10 seconds and under a gigabyte of
memory on the release machine. Each Windows build now also runs a second NSIS
build and builds `scripts/delta-tool`, so a release takes longer.

**Not yet proven.** The codec is tested in Rust (`src-tauri/src/delta_codec.rs`),
the release side in Node (`scripts/delta.test.mjs`), and the signatures were
checked with a throwaway key on a scratch build. No CI run has made a delta, and
no update has gone through the app's patch path on a real install.

An app built with an empty public key says updates aren't switched on, and never
asks the network. That is the state of a channel until its key is set.

**Not yet proven by a release.** The release side is checked as source: the
workflows parse, and `src/updater-manifest.test.js` and `scripts/*.test.mjs` pin
their wiring. No signed stable or nightly release has gone through it end to end.

## Feeds and pages

| | stable | nightly |
|---|---|---|
| feed (`latest.json`) | `https://github.com/morewebs/OpenRemote/releases/latest/download/latest.json` | `https://github.com/morewebs/OpenRemote/releases/download/nightly/latest.json` |
| Windows installer | `.../releases/latest/download/OpenRemote-Setup-x64.exe` | `.../releases/download/nightly/OpenRemote-Nightly-Setup-x64.exe` |
| Linux AppImage | `.../releases/latest/download/OpenRemote-x86_64.AppImage` | `.../releases/download/nightly/OpenRemote-Nightly-x86_64.AppImage` |
| release page (the badge opens it) | `.../releases/tag/vX.Y.Z` | `.../releases/tag/nightly` |

The feed is the one endpoint in `src-tauri/tauri.conf.json` (stable) or
`src-tauri/tauri.nightly.conf.json` (nightly). `latest.json` carries Tauri's
fields plus `page`, which the plugin ignores.

## Keys

Make one key pair per channel, and keep the private keys and their passwords
outside the repository:

```sh
npx tauri signer generate -w ~/.tauri/openremote-stable.key
npx tauri signer generate -w ~/.tauri/openremote-nightly.key
```

The command prints each public key. Losing a private key means the next release
cannot reach installed copies as an update, and users reinstall by hand.

| Channel | Public key, committed | GitHub secret: private key | GitHub secret: password |
|---|---|---|---|
| stable | `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` | `TAURI_SIGNING_PRIVATE_KEY` | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |
| nightly | `plugins.updater.pubkey` in `src-tauri/tauri.nightly.conf.json` | `NIGHTLY_TAURI_SIGNING_PRIVATE_KEY` | `NIGHTLY_TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |

The secret holding a private key is the key file's contents. The four secrets
live under Settings > Secrets and variables > Actions. The nightly build refuses
to run without its key.

## First nightly

1. Generate the nightly key pair, as above.
2. Put the nightly public key into `plugins.updater.pubkey` in
   `src-tauri/tauri.nightly.conf.json`, and commit it. Until then the nightly
   builds and publishes, but its app says updates aren't switched on.
3. Add the two `NIGHTLY_` secrets.
4. Actions > Nightly > Run workflow, on `main`. Check that the run passes, that
   the rolling `nightly` release holds both installers, their `.sig` files and
   `latest.json`, and that the feed URL above lists `windows-x86_64` and
   `linux-x86_64`.
5. Install the nightly installer. After the next push to main passes CI, check
   that the installed app offers the newer nightly.

## Stable release

A stable release is cut by a `vX.Y.Z` tag, as before, with two rules the release
job enforces in its preflight:

- The tag is `v` followed by the `version` in `src-tauri/tauri.conf.json`,
  exactly. Bump the file first, then tag.
- The tagged commit is on the default branch.

Only a tag with no pre-release suffix writes `latest.json`, because the stable
app reads `releases/latest`, which a pre-release must not move. A pre-release tag
still builds and carries its signatures.

## Nightly builds

`.github/workflows/nightly.yml` builds after CI passes on a push to `main`, and on
a manual run from `main`. It builds the Windows installer (NSIS) and the Linux
AppImage, signed with the nightly key.

- Version: `<base>-nightly.<stamp>`. The base is the `version` in
  `tauri.conf.json` with its patch number raised by one, and the stamp is the UTC
  build time as `YYYYMMDDHHmm`. For 0.5.0 at 02:15 UTC on 13 November 2026:
  `0.5.1-nightly.202611130215`.
- The rolling `nightly` release is re-pointed to the built commit, and its
  installers and `latest.json` are replaced.
- Each build is also a dated release, `nightly-<stamp>`. The newest 20 are kept;
  older ones are deleted with their tags.
- Both are prereleases, so no nightly ever becomes `releases/latest`.
- A failed build publishes nothing, so the last nightly stays up.

## Proving it

Before relying on a channel: install the build before the one you test, check
that the app offers the new version, that the daemon stops before the installer
runs, and that the app comes back on the new version with its chats.

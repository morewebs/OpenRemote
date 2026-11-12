# In-app updates

The desktop app looks for `latest.json` in the newest stable release
(`https://github.com/morewebs/OpenRemote/releases/latest/download/latest.json`,
the one endpoint in `src-tauri/tauri.conf.json`) once a day, the first look
thirty seconds after it starts (Settings > This install > Updates, where a
switch turns the daily look off). A newer version is installed only when the
user clicks Update and restart: the shell downloads it, verifies it against the
public key the app was built with, stops the daemon and waits for it to finish
(the installer replaces the daemon's binary too), installs, and restarts into
the new version. A download whose signature does not verify is refused before
anything is stopped; an install that fails starts the daemon again.

The look is the updater plugin's own request. The console and the shell add no
header, query or identifier to it.

Until the signing key exists the app says updates aren't switched on, asks
nothing of the network, and the release builds are exactly as before.

**Not yet proven by a test release.** The release side below has been checked
as source (the workflow parses, and `src/updater-manifest.test.js` pins its
wiring), but no signed release has gone through it end to end.

## Switching updates on (once)

1. Generate the key pair on your own machine. Keep the private key and its
   password out of the repository; losing them means users must reinstall by
   hand:

   ```sh
   npx tauri signer generate -w ~/.tauri/openremote-updates.key
   ```

2. Put the printed public key into `src-tauri/tauri.conf.json` at
   `plugins.updater.pubkey`, and commit that.
3. Add two repository secrets on GitHub: `TAURI_SIGNING_PRIVATE_KEY` (the
   private key file's contents) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

From then on the release workflow (`.github/workflows/release.yml`) builds with
`createUpdaterArtifacts` on, and each platform job stages its signature at the
release root beside the version-free installer it signs:
`OpenRemote-Setup-x64.exe.sig`, `OpenRemote-x86_64.AppImage.sig`, and the macOS
app archive `OpenRemote-universal.app.tar.gz` with its `.sig`. The `publish`
job writes `latest.json` from them (`scripts/updater-manifest.mjs`) for a stable
tag and ships it in the same release as the installers. The nightly carries the
signatures but no `latest.json`: the app reads `releases/latest`, which a
nightly never becomes.

## Proving it

Before relying on it: tag a test release with the key in place, install the
version before it, and check that the app offers the new version, that the
daemon stops before the installer runs, and that the app comes back on the new
version with its chats.

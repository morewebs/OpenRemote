// Builds the daemon and stages it as the Tauri sidecar, named with the
// host's target triple (Tauri's externalBin convention).
//   node scripts/sync-daemon.mjs            → debug daemon
//   OR_RELEASE=1 node scripts/sync-daemon.mjs → release daemon
//
// Also refreshes the dev sidecar copy under src-tauri/target when one is
// already there — `cargo tauri dev` only re-copies the sidecar when the
// app itself rebuilds, so a daemon-only change would otherwise leave the
// running dev app on the old binary until that happens.
import { execSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const release = !!process.env.OR_RELEASE
const profile = release ? 'release' : 'debug'

const triple =
  process.platform === 'win32'
    ? process.arch === 'arm64'
      ? 'aarch64-pc-windows-msvc'
      : 'x86_64-pc-windows-msvc'
    : process.platform === 'darwin'
      ? process.arch === 'arm64'
        ? 'aarch64-apple-darwin'
        : 'x86_64-apple-darwin'
      : process.arch === 'arm64'
        ? 'aarch64-unknown-linux-gnu'
        : 'x86_64-unknown-linux-gnu'

const ext = process.platform === 'win32' ? '.exe' : ''
const src = path.join(root, 'daemon', 'target', profile, `openremote-daemon${ext}`)
const dest = path.join(root, 'src-tauri', 'binaries', `openremote-daemon-${triple}${ext}`)

console.log(`building daemon (${profile})…`)
execSync(`cargo build -p openremote-daemon --locked${release ? ' --release' : ''}`, {
  cwd: path.join(root, 'daemon'),
  stdio: 'inherit',
})
mkdirSync(path.dirname(dest), { recursive: true })
copyFileSync(src, dest)
console.log(`staged ${dest}`)

// The dev build's own sidecar copy, if a dev build has been run before.
const devCopy = path.join(root, 'src-tauri', 'target', 'debug', `openremote-daemon${ext}`)
if (!release && existsSync(devCopy)) {
  try {
    copyFileSync(src, devCopy)
    console.log(`refreshed ${devCopy} (dev sidecar)`)
  } catch {
    // Locked (a dev app is running with it): the staged binaries/ copy is
    // the source of truth for the next app rebuild anyway.
    console.log(`dev sidecar ${devCopy} is locked — close the app and re-run`)
  }
}


// Builds the daemon and stages it as the Tauri sidecar, named with the
// host's target triple (Tauri's externalBin convention).
//   node scripts/sync-daemon.mjs            → debug daemon
//   OR_RELEASE=1 node scripts/sync-daemon.mjs → release daemon
import { execSync } from 'node:child_process'
import { copyFileSync, mkdirSync } from 'node:fs'
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

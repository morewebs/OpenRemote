// Writes the in-app updater's latest.json for a release: each platform's signed
// update and its signature, from the .sig files the release's builds staged
// beside their version-free installers (only builds made with a signing key
// have them). Two channels share one shape:
//
// - stable: the release workflow's `publish` job runs it for a vX.Y.Z tag and
//   ships latest.json in the same release, so `releases/latest/download/latest.json`
//   (the endpoint in tauri.conf.json) always describes the release it sits in.
// - nightly: .github/workflows/nightly.yml runs it for the rolling `nightly`
//   release, whose feed is releases/download/nightly/latest.json (the endpoint
//   in tauri.nightly.conf.json).
//
// Usage:
//   node scripts/updater-manifest.mjs <tag> <owner/repo> <sig-dir>
//   node scripts/updater-manifest.mjs --nightly <version> <owner/repo> <sig-dir>
//
// Every URL it writes points at this repository's own release, and the app
// verifies each download against the public key it was built with, so a
// manifest can name a file but never vouch for one. `page` is the release page
// the app's badge opens; Tauri's plugin ignores it.
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseNightlyVersion } from './channel.mjs'

// The version-free names the stable platform jobs stage (release.yml), and the
// updater's platform keys each one serves. One universal macOS app serves
// both Mac architectures.
export const UPDATE_ASSETS = {
  'OpenRemote-Setup-x64.exe': ['windows-x86_64'],
  'OpenRemote-x86_64.AppImage': ['linux-x86_64'],
  'OpenRemote-universal.app.tar.gz': ['darwin-x86_64', 'darwin-aarch64'],
}

// The nightly is a separate app with its own names, so they are their own map:
// a stable release never carries them, and the stable list stays what the
// stable publish job stages.
export const NIGHTLY_UPDATE_ASSETS = {
  'OpenRemote-Nightly-Setup-x64.exe': ['windows-x86_64'],
  'OpenRemote-Nightly-x86_64.AppImage': ['linux-x86_64'],
}

const OWNER_REPO = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/

/// The manifest for one release, from `sigs` ({ asset name: signature text }).
/// Stable: `tag` is vX.Y.Z. Nightly: `channel: 'nightly'` and `version` is the
/// nightly version string, and the release is the rolling `nightly` one.
/// Throws on anything that would make a manifest point somewhere else.
export function updaterManifest({ channel = 'stable', tag, version, repo, sigs, notes = '', date = new Date() }) {
  let release // the release tag the manifest's files live under
  let shownVersion
  if (channel === 'stable') {
    // A stable version only: latest.json sits at releases/latest, so a
    // pre-release named here would be offered to every installed copy.
    const v = /^v(\d+\.\d+\.\d+)$/.exec(tag ?? '')?.[1]
    if (!v) throw new Error(`not a stable version tag: ${tag}`)
    release = tag
    shownVersion = v
  } else if (channel === 'nightly') {
    // A nightly is the rolling release, and its version is the one the build
    // stamped. The version is checked, so a malformed one never reaches a feed.
    parseNightlyVersion(version)
    release = 'nightly'
    shownVersion = version
  } else {
    throw new Error(`unknown channel: ${channel}`)
  }
  if (!OWNER_REPO.test(repo ?? '')) throw new Error(`not an owner/repo: ${repo}`)
  const assets = channel === 'nightly' ? NIGHTLY_UPDATE_ASSETS : UPDATE_ASSETS
  const platforms = {}
  for (const [name, signature] of Object.entries(sigs)) {
    if (!Object.hasOwn(assets, name)) continue
    const text = String(signature).trim()
    if (!text) throw new Error(`${name}.sig is empty`)
    const url = `https://github.com/${repo}/releases/download/${release}/${encodeURIComponent(name)}`
    for (const key of assets[name]) platforms[key] = { signature: text, url }
  }
  return {
    version: shownVersion,
    notes: String(notes).slice(0, 4000),
    pub_date: date.toISOString(),
    platforms,
    page: `https://github.com/${repo}/releases/tag/${release}`,
  }
}

function main(argv) {
  const nightly = argv[0] === '--nightly'
  const [first, repo, dir] = nightly ? argv.slice(1) : argv
  if (!first || !repo || !dir) {
    console.error('usage: node scripts/updater-manifest.mjs <tag> <owner/repo> <sig-dir>')
    console.error('       node scripts/updater-manifest.mjs --nightly <version> <owner/repo> <sig-dir>')
    process.exit(2)
  }
  const sigs = {}
  for (const file of readdirSync(dir).filter((f) => f.endsWith('.sig'))) {
    sigs[file.slice(0, -'.sig'.length)] = readFileSync(join(dir, file), 'utf8')
  }
  let manifest
  if (nightly) {
    manifest = updaterManifest({ channel: 'nightly', version: first, repo, sigs })
  } else {
    const notesFile = join('release-notes', `${first}.md`)
    const notes = existsSync(notesFile) ? readFileSync(notesFile, 'utf8') : ''
    manifest = updaterManifest({ tag: first, repo, sigs, notes })
  }
  if (Object.keys(manifest.platforms).length === 0) {
    console.error(`no signed update in ${dir}`)
    process.exit(1)
  }
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2))

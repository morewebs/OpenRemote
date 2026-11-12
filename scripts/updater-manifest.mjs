// Writes the in-app updater's latest.json for a stable release: each
// platform's signed update and its signature, from the .sig files the
// release's desktop builds staged beside their version-free installers (only
// builds made with the signing key have them). The release workflow's
// `publish` job runs it and ships latest.json in the same release, so
// `releases/latest/download/latest.json` (the endpoint in tauri.conf.json)
// always describes the release it sits in.
//
// Usage: node scripts/updater-manifest.mjs <tag> <owner/repo> <sig-dir>
//
// Every URL it writes points at this repository's own release for this tag,
// and the app verifies each download against the public key it was built
// with, so a manifest can name a file but never vouch for one.
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

// The version-free names the platform jobs stage (release.yml), and the
// updater's platform keys each one serves. One universal macOS app serves
// both Mac architectures.
export const UPDATE_ASSETS = {
  'OpenRemote-Setup-x64.exe': ['windows-x86_64'],
  'OpenRemote-x86_64.AppImage': ['linux-x86_64'],
  'OpenRemote-universal.app.tar.gz': ['darwin-x86_64', 'darwin-aarch64'],
}

/// The manifest for `tag`, from `sigs` ({ asset name: signature text }).
/// Throws on anything that would make a manifest point somewhere else.
export function updaterManifest({ tag, repo, sigs, notes = '', date = new Date() }) {
  const version = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(tag ?? '')?.[1]
  if (!version) throw new Error(`not a stable version tag: ${tag}`)
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repo ?? '')) throw new Error(`not an owner/repo: ${repo}`)
  const platforms = {}
  for (const [name, signature] of Object.entries(sigs)) {
    const keys = UPDATE_ASSETS[name]
    if (!keys) continue
    const text = String(signature).trim()
    if (!text) throw new Error(`${name}.sig is empty`)
    const url = `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`
    for (const key of keys) platforms[key] = { signature: text, url }
  }
  return { version, notes: String(notes).slice(0, 4000), pub_date: date.toISOString(), platforms }
}

function main() {
  const [tag, repo, dir] = process.argv.slice(2)
  if (!tag || !repo || !dir) {
    console.error('usage: node scripts/updater-manifest.mjs <tag> <owner/repo> <sig-dir>')
    process.exit(2)
  }
  const sigs = {}
  for (const file of readdirSync(dir).filter((f) => f.endsWith('.sig'))) {
    sigs[file.slice(0, -'.sig'.length)] = readFileSync(join(dir, file), 'utf8')
  }
  const notesFile = join('release-notes', `${tag}.md`)
  const notes = existsSync(notesFile) ? readFileSync(notesFile, 'utf8') : ''
  const manifest = updaterManifest({ tag, repo, sigs, notes })
  if (Object.keys(manifest.platforms).length === 0) {
    console.error(`no signed update in ${dir}`)
    process.exit(1)
  }
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main()

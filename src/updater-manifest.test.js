// The release side of in-app updates: latest.json as the publish job writes
// it, and the workflow wiring that feeds it. The workflow has no runner here,
// so its wiring is pinned as source text (the YAML itself is parsed in review).

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { UPDATE_ASSETS, updaterManifest } from '../scripts/updater-manifest.mjs'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('latest.json names each platform\'s version-free update in this tag\'s release', () => {
  const m = updaterManifest({
    tag: 'v0.5.0',
    repo: 'morewebs/OpenRemote',
    sigs: {
      'OpenRemote-Setup-x64.exe': 'sig-win\n',
      'OpenRemote-x86_64.AppImage': 'sig-linux',
      'OpenRemote-universal.app.tar.gz': 'sig-mac',
      'OpenRemote-Setup-x64.msi': 'ignored: the msi is not the update',
    },
    notes: 'notes',
    date: new Date('2026-11-12T00:00:00Z'),
  })
  assert.equal(m.version, '0.5.0')
  assert.equal(m.pub_date, '2026-11-12T00:00:00.000Z')
  assert.deepEqual(Object.keys(m.platforms).sort(), ['darwin-aarch64', 'darwin-x86_64', 'linux-x86_64', 'windows-x86_64'])
  assert.deepEqual(m.platforms['windows-x86_64'], {
    signature: 'sig-win',
    url: 'https://github.com/morewebs/OpenRemote/releases/download/v0.5.0/OpenRemote-Setup-x64.exe',
  })
  assert.equal(m.platforms['darwin-aarch64'].url, m.platforms['darwin-x86_64'].url, 'one universal app')
  for (const entry of Object.values(m.platforms)) {
    assert.match(entry.url, /^https:\/\/github\.com\/morewebs\/OpenRemote\/releases\/download\/v0\.5\.0\//)
  }
})

test('a manifest that could point elsewhere is refused', () => {
  const sigs = { 'OpenRemote-Setup-x64.exe': 's' }
  assert.throws(() => updaterManifest({ tag: 'nightly', repo: 'morewebs/OpenRemote', sigs }), /stable version tag/)
  assert.throws(() => updaterManifest({ tag: 'v1.2', repo: 'morewebs/OpenRemote', sigs }), /stable version tag/)
  assert.throws(() => updaterManifest({ tag: 'v1.2.3', repo: 'evil.com/x/y', sigs }), /owner\/repo/)
  assert.throws(() => updaterManifest({ tag: 'v1.2.3', repo: 'a/b', sigs: { 'OpenRemote-Setup-x64.exe': ' ' } }), /empty/)
})

test('the platform jobs stage each signature at the release root beside its installer', () => {
  const wf = read('../.github/workflows/release.yml')
  for (const name of Object.keys(UPDATE_ASSETS)) {
    // Staged under the version-free name, and carried in the job's artifact,
    // whose root files are the ones publish ships.
    assert.ok(wf.includes(` ${name}.sig\n`), `${name}.sig is staged`)
    assert.ok(wf.includes(`\n            ${name}.sig\n`), `${name}.sig rides in the artifact`)
  }
  // The key comes from the secrets and nowhere else, and only switches signing on.
  const keyLines = wf.split('\n').filter((l) => /^\s+TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)?:/.test(l))
  assert.equal(keyLines.length, 6, 'three builds, two secrets each')
  for (const line of keyLines) assert.match(line, /\$\{\{ secrets\.TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)? \}\}$/)
  assert.equal((wf.match(/createUpdaterArtifacts":true/g) ?? []).length, 3)
})

test('publish writes latest.json in the same release, and there is no separate job', () => {
  const wf = read('../.github/workflows/release.yml')
  const jobs = [...wf.slice(wf.indexOf('\njobs:\n')).matchAll(/^ {2}([a-z-]+):\n/gm)].map((m) => m[1])
  assert.deepEqual(jobs, ['release', 'linux', 'android', 'macos', 'publish'])
  const publish = wf.slice(wf.indexOf('\n  publish:\n'))
  assert.match(publish, /node scripts\/updater-manifest\.mjs "\$TAG" "\$GITHUB_REPOSITORY" sigs > latest\.json/)
  assert.match(publish, /assets\+=\(latest\.json\)/)
  // Written before the release goes out, so latest.json is never a step behind.
  assert.ok(publish.indexOf('> latest.json') < publish.indexOf('gh release create nightly'))
  assert.ok(publish.indexOf('> latest.json') < publish.indexOf('gh release create "$TAG"'))
  // Only for a stable tag: the app reads releases/latest, never the nightly.
  assert.match(publish, /if \[ "\$TAG" != "nightly" \]; then\n\s+mkdir -p sigs/)
  // The signatures and the macOS archive are among the files publish ships.
  assert.match(publish, /-o -name '\*\.app\.tar\.gz' -o -name '\*\.sig'/)
})

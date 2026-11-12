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
  // A pre-release is never offered as an update: releases/latest would serve
  // it to every stable install.
  assert.throws(() => updaterManifest({ tag: 'v1.2.3-rc.1', repo: 'morewebs/OpenRemote', sigs }), /stable version tag/)
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
  // The key comes from the secrets and nowhere else, only switches signing on,
  // and never reaches a manual run: a workflow_dispatch can build any branch,
  // and a signed build of unreviewed code is an update the app would accept.
  const keyLines = wf.split('\n').filter((l) => /^\s+TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)?:/.test(l))
  assert.equal(keyLines.length, 6, 'three builds, two secrets each')
  for (const line of keyLines) {
    assert.match(
      line,
      /: \$\{\{ github\.event_name != 'workflow_dispatch' && secrets\.TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)? \|\| '' \}\}$/,
    )
  }
  assert.equal((wf.match(/createUpdaterArtifacts":true/g) ?? []).length, 3)
})

test('a build that had the key and made no signature fails its job', () => {
  const wf = read('../.github/workflows/release.yml')
  // Each signed build checks its own signature in the same step, while the
  // key is still in its env: a silent skip would ship a release the app
  // cannot update to.
  const builds = wf.split('      - name: tauri build\n').slice(1).map((b) => b.slice(0, b.indexOf('\n      - ')))
  assert.equal(builds.length, 3)
  const wanted = [
    'src-tauri/target/release/bundle/nsis/*.exe.sig',
    'src-tauri/target/release/bundle/appimage/*.AppImage.sig',
    'src-tauri/target/universal-apple-darwin/release/bundle/macos/*.app.tar.gz.sig',
  ]
  builds.forEach((step, i) => {
    assert.ok(step.includes(`compgen -G '${wanted[i]}' > /dev/null`), `build ${i} checks ${wanted[i]}`)
    assert.match(step, /no update signature[^\n]*\n\s+exit 1/, `build ${i} fails without it`)
  })
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
  // Only for a stable version tag: the app reads releases/latest, never the
  // nightly and never a pre-release. The gate's own pattern is run here.
  const gate = /if \[\[ "\$TAG" =~ (\^[^ ]+\$) \]\]; then\n\s+mkdir -p sigs/.exec(publish)
  assert.ok(gate, 'publish gates latest.json on a version pattern')
  const stable = new RegExp(gate[1])
  assert.ok(stable.test('v1.2.3') && stable.test('v10.20.30'))
  for (const tag of ['nightly', 'v1.2.3-rc.1', 'v1.2.3-beta', 'v1.2', '1.2.3', 'v1.2.3.4']) {
    assert.ok(!stable.test(tag), `${tag} gets no latest.json`)
  }
  // A stable re-run with no signatures, over a release that already has a
  // latest.json, refuses instead of shipping installers that latest.json does
  // not describe (and deletes nothing from a live release).
  const refuse = publish.indexOf('already has latest.json')
  assert.ok(refuse > 0, 'the stale latest.json case is named')
  assert.match(publish.slice(refuse - 400, refuse + 200), /grep -qxF latest\.json <<< "\$existing"[\s\S]*exit 1/)
  assert.ok(refuse < publish.indexOf('gh release create "$TAG"'), 'refused before anything is published')
  assert.doesNotMatch(publish, /delete-asset/)
  // The signatures and the macOS archive are among the files publish ships.
  assert.match(publish, /-o -name '\*\.app\.tar\.gz' -o -name '\*\.sig'/)
})

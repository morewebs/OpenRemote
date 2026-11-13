import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { NIGHTLY_UPDATE_ASSETS, UPDATE_ASSETS, updaterManifest } from './updater-manifest.mjs'

const REPO = 'morewebs/OpenRemote'
const DATE = new Date('2026-11-13T02:15:00Z')
const NIGHTLY = '0.5.1-nightly.202611130215'
const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('a stable manifest keeps its exact shape and adds only page', () => {
  const m = updaterManifest({
    tag: 'v0.5.0',
    repo: REPO,
    sigs: {
      'OpenRemote-Setup-x64.exe': 'sig-win\n',
      'OpenRemote-x86_64.AppImage': 'sig-linux',
      'OpenRemote-Setup-x64.msi': 'not an update',
    },
    notes: 'notes',
    date: DATE,
  })
  assert.deepEqual(Object.keys(m), ['version', 'notes', 'pub_date', 'platforms', 'page'])
  assert.equal(m.version, '0.5.0')
  assert.equal(m.notes, 'notes')
  assert.equal(m.pub_date, '2026-11-13T02:15:00.000Z')
  assert.deepEqual(m.platforms['windows-x86_64'], {
    signature: 'sig-win',
    url: 'https://github.com/morewebs/OpenRemote/releases/download/v0.5.0/OpenRemote-Setup-x64.exe',
  })
  assert.deepEqual(Object.keys(m.platforms).sort(), ['linux-x86_64', 'windows-x86_64'])
  assert.equal(m.page, 'https://github.com/morewebs/OpenRemote/releases/tag/v0.5.0')
})

test('a stable manifest with no notes still has the same shape', () => {
  const m = updaterManifest({ tag: 'v0.5.0', repo: REPO, sigs: { 'OpenRemote-Setup-x64.exe': 's' }, date: DATE })
  assert.deepEqual(Object.keys(m), ['version', 'notes', 'pub_date', 'platforms', 'page'])
  assert.equal(m.notes, '')
})

test('a stable manifest refuses any tag that is not vX.Y.Z', () => {
  const sigs = { 'OpenRemote-Setup-x64.exe': 's' }
  for (const tag of ['nightly', 'nightly-202611130215', 'v1.2', '1.2.3', 'v1.2.3.4', 'v1.2.3-rc.1', 'v']) {
    assert.throws(() => updaterManifest({ tag, repo: REPO, sigs }), /stable version tag/, tag)
  }
})

test('a nightly manifest names the rolling release and its own assets only', () => {
  const m = updaterManifest({
    channel: 'nightly',
    version: NIGHTLY,
    repo: REPO,
    sigs: {
      'OpenRemote-Nightly-Setup-x64.exe': 'sig-win',
      'OpenRemote-Nightly-x86_64.AppImage': 'sig-linux',
      'OpenRemote-Setup-x64.exe': 'the stable installer is not this feed',
      'OpenRemote-universal.app.tar.gz': 'no macOS in the nightly',
    },
    date: DATE,
  })
  assert.deepEqual(Object.keys(m), ['version', 'notes', 'pub_date', 'platforms', 'page'])
  assert.equal(m.version, NIGHTLY)
  assert.deepEqual(Object.keys(m.platforms).sort(), ['linux-x86_64', 'windows-x86_64'])
  assert.deepEqual(m.platforms['windows-x86_64'], {
    signature: 'sig-win',
    url: 'https://github.com/morewebs/OpenRemote/releases/download/nightly/OpenRemote-Nightly-Setup-x64.exe',
  })
  assert.equal(m.platforms['linux-x86_64'].url, 'https://github.com/morewebs/OpenRemote/releases/download/nightly/OpenRemote-Nightly-x86_64.AppImage')
  assert.equal(m.page, 'https://github.com/morewebs/OpenRemote/releases/tag/nightly')
})

test('a nightly manifest refuses a version that is not a nightly version', () => {
  const sigs = { 'OpenRemote-Nightly-Setup-x64.exe': 's' }
  for (const version of ['0.5.0', 'v0.5.1-nightly.202611130215', '0.5.1-nightly.2026', '0.5.1-rc.1', undefined]) {
    assert.throws(
      () => updaterManifest({ channel: 'nightly', version, repo: REPO, sigs }),
      /not a nightly version/,
      String(version),
    )
  }
})

test('an unknown channel is refused', () => {
  assert.throws(() => updaterManifest({ channel: 'beta', tag: 'v0.5.0', repo: REPO, sigs: {} }), /unknown channel/)
})

test('a manifest that could point elsewhere is refused on either channel', () => {
  const sigs = { 'OpenRemote-Setup-x64.exe': 's' }
  assert.throws(() => updaterManifest({ tag: 'v1.2.3', repo: 'evil.com/x/y', sigs }), /owner\/repo/)
  assert.throws(
    () => updaterManifest({ channel: 'nightly', version: NIGHTLY, repo: 'evil.com/x/y', sigs: { 'OpenRemote-Nightly-Setup-x64.exe': 's' } }),
    /owner\/repo/,
  )
  assert.throws(
    () => updaterManifest({ tag: 'v1.2.3', repo: 'a/b', sigs: { 'OpenRemote-Setup-x64.exe': ' ' } }),
    /empty/,
  )
})

test('the nightly names and the stable names do not overlap', () => {
  for (const name of Object.keys(NIGHTLY_UPDATE_ASSETS)) {
    assert.equal(name in UPDATE_ASSETS, false, name)
    assert.match(name, /^OpenRemote-Nightly-/)
  }
})

test('the nightly workflow stages every nightly name the manifest maps', () => {
  const wf = read('../.github/workflows/nightly.yml').replace(/\r\n/g, '\n')
  for (const name of Object.keys(NIGHTLY_UPDATE_ASSETS)) {
    assert.ok(wf.includes(`staged/${name}`), `${name} is staged`)
    assert.ok(wf.includes(`assets/${name}.sig`), `${name}.sig is read by the feed`)
  }
})

test('the release workflow has no nightly schedule', () => {
  const wf = read('../.github/workflows/release.yml').replace(/\r\n/g, '\n')
  assert.doesNotMatch(wf, /^ {2}schedule:/m)
  assert.doesNotMatch(wf, /cron:/)
})

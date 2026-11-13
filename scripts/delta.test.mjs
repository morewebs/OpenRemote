// The release side of the in-app delta updates: the uncompressed build's
// config, the signing arguments, the delta record, and the manifest entry the
// app reads. The codec itself is tested in Rust (src-tauri/src/delta_codec.rs).

import assert from 'node:assert/strict'
import test from 'node:test'
import { deltaEntries, updaterManifest } from './updater-manifest.mjs'
import { deltaRecord, signArgs, uncompressedConfig } from './delta.mjs'

const repo = 'morewebs/OpenRemote'

test('the uncompressed build changes only the installer compression', () => {
  const base = {
    productName: 'OpenRemote',
    bundle: { active: true, targets: ['nsis'], windows: { nsis: { installMode: 'passive' } } },
  }
  const out = uncompressedConfig(base)
  assert.equal(out.bundle.windows.nsis.compression, 'none')
  assert.equal(out.bundle.windows.nsis.installMode, 'passive', 'the rest of the NSIS settings stay')
  assert.deepEqual(out.bundle.targets, ['nsis'])
  assert.equal(base.bundle.windows.nsis.compression, undefined, 'the input is not changed')
})

test('a signature is made for one version, and the version is bound into it', () => {
  assert.deepEqual(signArgs({ file: 'x.update.zst', version: '0.5.1' }), [
    'signer', 'sign', '--app-version', '0.5.1', 'x.update.zst',
  ])
  assert.deepEqual(signArgs({ file: 'x', version: '0.5.1-nightly.202611130215' }).slice(2, 4), [
    '--app-version', '0.5.1-nightly.202611130215',
  ])
  assert.throws(() => signArgs({ file: 'x', version: 'v0.5.1' }), /not a version/)
  assert.throws(() => signArgs({ file: 'x', version: '0.5.1; rm' }), /not a version/)
})

test('a patch record names the version it is made from; a full-only record names none', () => {
  const withPatch = deltaRecord({
    version: '0.5.1', stem: 'OpenRemote-Setup-x64', from: '0.5.0', installerSignature: ' sig \n', hasPatch: true,
  })
  assert.deepEqual(withPatch, {
    version: '0.5.1',
    full: 'OpenRemote-Setup-x64.update.zst',
    patch: 'OpenRemote-Setup-x64.patch.zst',
    from: '0.5.0',
    installer_signature: 'sig',
  })
  const fullOnly = deltaRecord({ version: '0.5.1', stem: 'S', installerSignature: 'sig', hasPatch: false })
  assert.equal(fullOnly.patch, null)
  assert.equal(fullOnly.from, null)
  assert.throws(
    () => deltaRecord({ version: '0.5.1', stem: 'S', installerSignature: 'sig', hasPatch: true }),
    /a patch needs/,
  )
})

// What the publish step reads: the sig-dir holds the delta's signed files and
// delta.json, next to the installers' own signatures.
const sigs = {
  'OpenRemote-Setup-x64.exe': 'sig-installer',
  'OpenRemote-Setup-x64.update.zst': 'sig-full\n',
  'OpenRemote-Setup-x64.patch.zst': 'sig-patch\n',
}
const delta = {
  version: '0.5.1',
  full: 'OpenRemote-Setup-x64.update.zst',
  patch: 'OpenRemote-Setup-x64.patch.zst',
  from: '0.5.0',
  installer_signature: 'sig-raw-installer',
}

test('a stable manifest carries the patch, the full package and the installer signature', () => {
  const m = updaterManifest({ tag: 'v0.5.1', repo, sigs, delta })
  assert.deepEqual(m.delta.installer, { signature: 'sig-raw-installer' })
  assert.deepEqual(m.delta.full, {
    url: 'https://github.com/morewebs/OpenRemote/releases/download/v0.5.1/OpenRemote-Setup-x64.update.zst',
    signature: 'sig-full',
  })
  assert.equal(m.delta.from, '0.5.0')
  assert.equal(m.delta.patch.url, 'https://github.com/morewebs/OpenRemote/releases/download/v0.5.1/OpenRemote-Setup-x64.patch.zst')
  assert.equal(m.delta.patch.signature, 'sig-patch')
  // The platform entry is unchanged: an app with no delta support still updates.
  assert.equal(m.platforms['windows-x86_64'].signature, 'sig-installer')
})

test('a nightly manifest names the rolling release for its delta files', () => {
  const version = '0.5.1-nightly.202611130215'
  const m = updaterManifest({
    channel: 'nightly',
    version,
    repo,
    sigs: { 'OpenRemote-Nightly-Setup-x64.update.zst': 's' },
    delta: { ...delta, version, full: 'OpenRemote-Nightly-Setup-x64.update.zst', patch: null, from: null },
  })
  assert.equal(m.delta.full.url, 'https://github.com/morewebs/OpenRemote/releases/download/nightly/OpenRemote-Nightly-Setup-x64.update.zst')
  assert.equal(m.delta.patch, undefined, 'no patch when the build made none')
  assert.equal(m.delta.from, undefined)
})

test('a manifest without a delta file is the feed it was before', () => {
  const m = updaterManifest({ tag: 'v0.5.1', repo, sigs: { 'OpenRemote-Setup-x64.exe': 's' } })
  assert.equal('delta' in m, false)
})

test('a delta that does not match its release, or lacks a file, is refused', () => {
  assert.throws(() => updaterManifest({ tag: 'v0.5.2', repo, sigs, delta }), /delta.json is for 0.5.1/)
  assert.throws(
    () => deltaEntries({ delta, repo, release: 'v0.5.1', sigs: { ...sigs, 'OpenRemote-Setup-x64.patch.zst': ' ' }, version: '0.5.1' }),
    /has no signature/,
  )
  assert.throws(
    () => deltaEntries({ delta: { ...delta, installer_signature: '' }, repo, release: 'v0.5.1', sigs, version: '0.5.1' }),
    /no installer signature/,
  )
  assert.throws(
    () => deltaEntries({ delta: { ...delta, from: undefined }, repo, release: 'v0.5.1', sigs, version: '0.5.1' }),
    /needs the version it is made from/,
  )
  assert.throws(
    () => deltaEntries({ delta: { ...delta, from: 'x; rm' }, repo, release: 'v0.5.1', sigs, version: '0.5.1' }),
    /needs the version it is made from/,
  )
})

test('a delta file name cannot point the manifest at another release', () => {
  const m = updaterManifest({
    tag: 'v0.5.1',
    repo,
    sigs: { ...sigs, 'a/../b.update.zst': 'sig' },
    delta: { ...delta, full: 'a/../b.update.zst' },
  })
  assert.match(m.delta.full.url, /\/releases\/download\/v0\.5\.1\/a%2F\.\.%2Fb\.update\.zst$/, 'the slashes are encoded, so the name stays one path segment')
})

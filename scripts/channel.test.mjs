import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import test from 'node:test'
import {
  mergeConfig,
  nightlyConfig,
  nightlyVersion,
  parseNightlyVersion,
  planNightly,
  stampFor,
} from './channel.mjs'

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '..')
const readRepoJson = (path) => JSON.parse(readFileSync(join(root, path), 'utf8'))

const NIGHTLY_FEED = 'https://github.com/morewebs/OpenRemote/releases/download/nightly/latest.json'
const DATE = new Date('2026-11-13T02:15:00Z')

test('the stamp is the UTC minute', () => {
  assert.equal(stampFor(DATE), '202611130215')
  assert.equal(stampFor(new Date('2026-12-31T23:59:59Z')), '202612312359')
  assert.equal(stampFor(new Date('2027-01-01T00:00:00Z')), '202701010000')
})

test('the stamp reads UTC even in a zone far from it', () => {
  // A child process takes its zone at start, so the zone is set for it alone.
  // Kiritimati is UTC+14: local hours would read 16:15 for the same instant.
  const url = pathToFileURL(join(here, 'channel.mjs')).href
  const script = `const { stampFor } = await import(${JSON.stringify(url)}); process.stdout.write(stampFor(new Date("2026-11-13T02:15:00Z")))`
  const out = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
    env: { ...process.env, TZ: 'Pacific/Kiritimati' },
    encoding: 'utf8',
  })
  assert.equal(out, '202611130215')
})

test('an invalid date is refused', () => {
  assert.throws(() => stampFor(new Date('not a date')), /valid Date/)
  assert.throws(() => stampFor('2026-11-13'), /valid Date/)
})

test('a nightly version is the base with its patch raised by one, and the stamp', () => {
  assert.equal(nightlyVersion('0.5.0', DATE), '0.5.1-nightly.202611130215')
  assert.equal(nightlyVersion('1.9.9', DATE), '1.9.10-nightly.202611130215')
})

test('a base that is not a plain X.Y.Z version is refused', () => {
  for (const bad of ['0.5', 'v0.5.0', '0.5.0-rc.1', '0.5.0.1', '', undefined]) {
    assert.throws(() => nightlyVersion(bad, DATE), /not a plain X\.Y\.Z/, String(bad))
  }
})

test('a nightly version parses, and every malformed one is refused', () => {
  assert.deepEqual(parseNightlyVersion('0.5.1-nightly.202611130215'), { base: '0.5.1', stamp: '202611130215' })
  const bad = [
    '0.5.1',
    '0.5.1-nightly.20261113021', // eleven digits
    '0.5.1-nightly.2026111302150', // thirteen digits
    '0.5.1-nightly.202613130215', // month 13
    '0.5.1-nightly.202611320215', // day 32
    '0.5.1-nightly.202611132415', // hour 24
    '0.5.1-nightly.202611130260', // minute 60
    'v0.5.1-nightly.202611130215',
    '0.5.1-nightly.202611130215.1',
    '0.5.1-beta.202611130215',
  ]
  for (const version of bad) {
    assert.throws(() => parseNightlyVersion(version), /not a nightly version/, version)
  }
})

test('the plan is one version and the stamp of the same instant', () => {
  const plan = planNightly({ baseVersion: '0.5.0', date: DATE })
  assert.deepEqual(plan, { version: '0.5.1-nightly.202611130215', stamp: '202611130215' })
})

// Semver precedence for the forms this channel makes: X.Y.Z with an optional
// dot-separated pre-release. A release sorts after its own pre-releases;
// numeric identifiers compare as numbers and sort before alphanumeric ones.
function parseSemver(v) {
  const dash = v.indexOf('-')
  const core = (dash < 0 ? v : v.slice(0, dash)).split('.').map(Number)
  const pre = dash < 0 ? [] : v.slice(dash + 1).split('.')
  return { core, pre }
}

function compareSemver(a, b) {
  const x = parseSemver(a)
  const y = parseSemver(b)
  for (let i = 0; i < 3; i++) {
    if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i]
  }
  if (x.pre.length === 0 || y.pre.length === 0) {
    return (x.pre.length === 0 ? 1 : 0) - (y.pre.length === 0 ? 1 : 0)
  }
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i]
    const q = y.pre[i]
    if (p === undefined) return -1
    if (q === undefined) return 1
    const pNum = /^\d+$/.test(p)
    const qNum = /^\d+$/.test(q)
    if (pNum && qNum) {
      if (Number(p) !== Number(q)) return Number(p) - Number(q)
    } else if (pNum !== qNum) {
      return pNum ? -1 : 1
    } else if (p !== q) {
      return p < q ? -1 : 1
    }
  }
  return 0
}

test('nightly versions sort as semver pre-releases, between the stables around them', () => {
  const order = [
    '0.4.9',
    '0.5.0',
    '0.5.1-nightly.202611130215',
    '0.5.1-nightly.202611131000',
    '0.5.1-nightly.202611200000',
    '0.5.1',
    '0.5.2-nightly.202611140000',
  ]
  const scrambled = [order[4], order[0], order[6], order[2], order[5], order[1], order[3]]
  assert.deepEqual([...scrambled].sort(compareSemver), order)
  // A nightly is newer than the stable it follows and older than the next one.
  assert.ok(compareSemver('0.5.1-nightly.202611130215', '0.5.0') > 0)
  assert.ok(compareSemver('0.5.1-nightly.202611130215', '0.5.1') < 0)
})

test('mergeConfig merges objects, replaces arrays and scalars, and removes on null', () => {
  const base = { a: { b: 1, c: 2 }, list: [1, 2], gone: 1, keep: 'x' }
  const merged = mergeConfig(base, { a: { c: 3, d: 4 }, list: [9], gone: null })
  assert.deepEqual(merged, { a: { b: 1, c: 3, d: 4 }, list: [9], keep: 'x' })
  assert.deepEqual(base, { a: { b: 1, c: 2 }, list: [1, 2], gone: 1, keep: 'x' }, 'the base is not mutated')
})

test('the nightly config takes the overlay identity, the nightly version and the artifacts flag', () => {
  const base = readRepoJson('src-tauri/tauri.conf.json')
  const overlay = readRepoJson('src-tauri/tauri.nightly.conf.json')
  const before = JSON.stringify(base)
  const config = nightlyConfig({ base, overlay, version: '0.5.1-nightly.202611130215' })
  assert.equal(config.productName, 'OpenRemote Nightly')
  assert.equal(config.identifier, 'space.moreweb.openremote.nightly')
  assert.equal(config.version, '0.5.1-nightly.202611130215')
  assert.deepEqual(config.plugins.updater.endpoints, [NIGHTLY_FEED])
  assert.equal(config.bundle.createUpdaterArtifacts, true)
  // Everything the overlay does not name is the stable config, unchanged.
  assert.deepEqual(config.build, base.build)
  assert.deepEqual(config.app, base.app)
  assert.equal(config.bundle.copyright, base.bundle.copyright)
  assert.equal(JSON.stringify(base), before, 'the base is not mutated')
})

test('the overlay on disk never carries a version, since the build sets it', () => {
  const overlay = readRepoJson('src-tauri/tauri.nightly.conf.json')
  assert.equal('version' in overlay, false)
})

test('a nightly config that shares the stable identity or feed is refused', () => {
  const base = readRepoJson('src-tauri/tauri.conf.json')
  const overlay = readRepoJson('src-tauri/tauri.nightly.conf.json')
  const version = '0.5.1-nightly.202611130215'
  assert.throws(
    () => nightlyConfig({ base, overlay: { ...overlay, identifier: base.identifier }, version }),
    /identifier must differ/,
  )
  assert.throws(
    () => nightlyConfig({ base, overlay: { ...overlay, productName: base.productName }, version }),
    /product name must differ/,
  )
  const stableFeed = { plugins: { updater: { endpoints: [base.plugins.updater.endpoints[0]] } } }
  assert.throws(() => nightlyConfig({ base, overlay: { ...overlay, ...stableFeed }, version }), /nightly feed/)
  assert.throws(
    () => nightlyConfig({ base, overlay: { ...overlay, plugins: { updater: { endpoints: [] } } }, version }),
    /nightly feed/,
  )
  assert.throws(() => nightlyConfig({ base, overlay, version: '0.5.0' }), /not a nightly version/)
})

test('the CLI plans a version and stamp, and writes the config to the path it is given', () => {
  const plan = execFileSync(process.execPath, [join(root, 'scripts', 'channel.mjs'), 'plan'], { encoding: 'utf8' })
  const m = /^version=(\d+\.\d+\.\d+-nightly\.(\d{12}))\nstamp=(\d{12})\n$/.exec(plan)
  assert.ok(m, `plan output: ${JSON.stringify(plan)}`)
  assert.equal(m[2], m[3], 'the version and the stamp agree')

  const dir = mkdtempSync(join(tmpdir(), 'channel-test-'))
  try {
    const out = join(dir, 'nested', 'nightly.json')
    execFileSync(process.execPath, [join(root, 'scripts', 'channel.mjs'), 'config', '0.5.1-nightly.202611130215', out], {
      encoding: 'utf8',
    })
    const written = JSON.parse(readFileSync(out, 'utf8'))
    assert.equal(written.version, '0.5.1-nightly.202611130215')
    assert.equal(written.identifier, 'space.moreweb.openremote.nightly')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

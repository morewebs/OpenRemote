// The nightly channel's rules, and the one config a nightly build is given.
//
// A nightly is named `<base>-nightly.<stamp>`. The base is the version in
// src-tauri/tauri.conf.json with its patch number raised by one, so a nightly
// sorts after the stable it follows and before the stable after that. The
// stamp is the UTC minute the build was planned at (YYYYMMDDHHmm): UTC, so the
// same instant reads the same on every runner and in every zone.
//
// The nightly identity (product name, identifier, feed, updater key) is the
// overlay in src-tauri/tauri.nightly.conf.json. Its public key is blank until
// the nightly key pair exists, and a build with a blank key never checks for
// updates. docs/releasing-updates.md says where the key goes.
//
// Usage:
//   node scripts/channel.mjs plan                      prints version= and stamp= lines
//   node scripts/channel.mjs config <version> <out>    writes the full nightly config
//
// The build is given one full config file rather than several --config
// values. The file is what the build reads, so the config can be inspected
// before the build runs, and nothing depends on how the CLI merges several.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const STABLE_CONFIG = fileURLToPath(new URL('../src-tauri/tauri.conf.json', import.meta.url))
const NIGHTLY_OVERLAY = fileURLToPath(new URL('../src-tauri/tauri.nightly.conf.json', import.meta.url))

// The feed a nightly reads: this repository's rolling nightly release, and
// nothing else. A nightly that named the stable feed would be offered stable
// builds.
const NIGHTLY_FEED = /^https:\/\/github\.com\/[^/]+\/[^/]+\/releases\/download\/nightly\/latest\.json$/

const PLAIN_VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/
const NIGHTLY_VERSION =
  /^((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))-nightly\.(\d{4}(?:0[1-9]|1[0-2])(?:0[1-9]|[12]\d|3[01])(?:[01]\d|2[0-3])[0-5]\d)$/

/// The UTC stamp (YYYYMMDDHHmm) of `date`.
export function stampFor(date) {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) {
    throw new Error('stampFor needs a valid Date')
  }
  const two = (n) => String(n).padStart(2, '0')
  return [
    String(date.getUTCFullYear()).padStart(4, '0'),
    two(date.getUTCMonth() + 1),
    two(date.getUTCDate()),
    two(date.getUTCHours()),
    two(date.getUTCMinutes()),
  ].join('')
}

/// The nightly version for a stable base version planned at `date`.
export function nightlyVersion(baseVersion, date) {
  const m = PLAIN_VERSION.exec(String(baseVersion))
  if (!m) throw new Error(`not a plain X.Y.Z version: ${baseVersion}`)
  return `${m[1]}.${m[2]}.${Number(m[3]) + 1}-nightly.${stampFor(date)}`
}

/// { base, stamp } for a nightly version; throws on anything else.
export function parseNightlyVersion(version) {
  const m = NIGHTLY_VERSION.exec(String(version))
  if (!m) throw new Error(`not a nightly version (X.Y.Z-nightly.YYYYMMDDHHmm): ${version}`)
  return { base: m[1], stamp: m[2] }
}

/// The version and stamp a build planned at `date` carries.
export function planNightly({ baseVersion, date = new Date() }) {
  return { version: nightlyVersion(baseVersion, date), stamp: stampFor(date) }
}

const isObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v)

/// JSON Merge Patch (RFC 7396): objects merge key by key, arrays and scalars
/// replace what was there, and a null removes its key. Nothing is mutated.
export function mergeConfig(base, overlay) {
  if (!isObject(overlay)) return overlay
  const out = isObject(base) ? { ...base } : {}
  for (const [key, value] of Object.entries(overlay)) {
    if (value === null) delete out[key]
    else out[key] = mergeConfig(out[key], value)
  }
  return out
}

/// The full config a nightly build runs with: the stable config with the
/// nightly overlay on top, the nightly version, and the updater artifacts that
/// the nightly's signature check needs. Refuses an overlay that would give the
/// nightly the stable identity or the stable feed.
export function nightlyConfig({ base, overlay, version }) {
  parseNightlyVersion(version)
  const merged = mergeConfig(base, overlay)
  if (merged.identifier === base.identifier) {
    throw new Error('the nightly identifier must differ from the stable one, or the two installs share data')
  }
  if (merged.productName === base.productName) {
    throw new Error('the nightly product name must differ from the stable one')
  }
  const feeds = merged.plugins?.updater?.endpoints ?? []
  if (feeds.length === 0 || !feeds.every((url) => NIGHTLY_FEED.test(url))) {
    throw new Error('every nightly updater endpoint must be the nightly feed (releases/download/nightly/latest.json)')
  }
  return {
    ...merged,
    version,
    bundle: { ...merged.bundle, createUpdaterArtifacts: true },
  }
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function main(argv) {
  const [command, ...rest] = argv
  if (command === 'plan' && rest.length === 0) {
    const { version, stamp } = planNightly({ baseVersion: readJson(STABLE_CONFIG).version })
    process.stdout.write(`version=${version}\nstamp=${stamp}\n`)
    return
  }
  if (command === 'config' && rest.length === 2) {
    const [version, out] = rest
    const config = nightlyConfig({
      base: readJson(STABLE_CONFIG),
      overlay: readJson(NIGHTLY_OVERLAY),
      version,
    })
    mkdirSync(dirname(out), { recursive: true })
    writeFileSync(out, `${JSON.stringify(config, null, 2)}\n`)
    process.stdout.write(`wrote ${out} for ${version}\n`)
    return
  }
  console.error('usage: node scripts/channel.mjs plan | node scripts/channel.mjs config <version> <out>')
  process.exit(2)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2))

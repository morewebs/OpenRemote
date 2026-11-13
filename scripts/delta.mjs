// The release side of in-app updates, for the Windows jobs that build an
// installer. Each release ships three signed files for the app to choose from,
// and the app picks the smallest one it can use:
//
// - a patch from the previous release's installer (the common case: a few
//   kilobytes to a few megabytes, depending on what changed);
// - the whole new installer, packed (a fallback for an app with no stored base);
// - and the release's own LZMA installer, which a fresh download uses and an
//   app that has nothing else can still update to.
//
// The patch and the full package are made from the UNCOMPRESSED installer. An
// LZMA installer changes almost everywhere when one byte of the app changes, so
// two of them cannot be patched small; the uncompressed ones can. The
// uncompressed build is a second NSIS build of the same app (see `uncompressed`).
//
// Usage:
//   node scripts/delta.mjs uncompressed <config-in> <config-out>
//       the same Tauri config with the NSIS installer left uncompressed
//   node scripts/delta.mjs make --tool <delta-tool> --stem <name> --version <v>
//       --installer <uncompressed.exe> --out <dir>
//       [--base <previous uncompressed.exe> --from <previous version>]
//       makes <stem>.update.zst, <stem>.patch.zst (with --base), and delta.json
//       in <dir>, and signs each file with TAURI_SIGNING_PRIVATE_KEY.
//
// The signing key comes from the environment, as `tauri signer sign` reads it.
// Nothing here prints a key or a password.
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { mergeConfig } from './channel.mjs'

const TAURI_CLI = fileURLToPath(new URL('../node_modules/@tauri-apps/cli/tauri.js', import.meta.url))

// The one setting this build changes: the NSIS installer stores its files as
// they are, so a change to the app is a change in place, not a change to the
// whole compressed stream.
const UNCOMPRESSED = { bundle: { windows: { nsis: { compression: 'none' } } } }

const VERSION = /^\d+\.\d+\.\d+(?:-nightly\.\d{12})?$/

/// The Tauri config with the NSIS installer left uncompressed.
export function uncompressedConfig(config) {
  return mergeConfig(config, UNCOMPRESSED)
}

/// The arguments that sign one file for one version. The version is bound into
/// the signature, so the app can refuse a signature made for another release.
export function signArgs({ file, version }) {
  if (!VERSION.test(version)) throw new Error(`not a version to sign for: ${version}`)
  return ['signer', 'sign', '--app-version', version, file]
}

/// The delta.json a release's publish step reads: which files make up the delta,
/// and the signature of the uncompressed installer they reproduce.
export function deltaRecord({ version, stem, from, installerSignature, hasPatch }) {
  if (!VERSION.test(version)) throw new Error(`not a version: ${version}`)
  if (hasPatch && !VERSION.test(from ?? '')) throw new Error('a patch needs the version it is made from')
  return {
    version,
    full: `${stem}.update.zst`,
    patch: hasPatch ? `${stem}.patch.zst` : null,
    from: hasPatch ? from : null,
    installer_signature: String(installerSignature).trim(),
  }
}

function run(command, args) {
  const result = spawnSync(command, args, { stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${basename(command)} failed (exit ${result.status})`)
}

function sign(file, version) {
  run(process.execPath, [TAURI_CLI, ...signArgs({ file, version })])
  return readFileSync(`${file}.sig`, 'utf8')
}

function argValue(args, name) {
  const at = args.indexOf(name)
  if (at === -1) return undefined
  const value = args[at + 1]
  if (value === undefined || value.startsWith('--')) throw new Error(`${name} needs a value`)
  return value
}

function main(argv) {
  const [command, ...rest] = argv
  if (command === 'uncompressed' && rest.length === 2) {
    const [input, output] = rest
    const config = uncompressedConfig(JSON.parse(readFileSync(input, 'utf8')))
    writeFileSync(output, `${JSON.stringify(config, null, 2)}\n`)
    process.stdout.write(`wrote ${output} (installer uncompressed)\n`)
    return
  }
  if (command === 'make') {
    const tool = argValue(rest, '--tool')
    const stem = argValue(rest, '--stem')
    const version = argValue(rest, '--version')
    const installer = argValue(rest, '--installer')
    const out = argValue(rest, '--out')
    const base = argValue(rest, '--base')
    const from = argValue(rest, '--from')
    if (!tool || !stem || !version || !installer || !out) {
      throw new Error('make needs --tool, --stem, --version, --installer and --out')
    }
    if (base && !from) throw new Error('--base needs --from, the version the base is from')
    mkdirSync(out, { recursive: true })

    const full = join(out, `${stem}.update.zst`)
    run(tool, ['pack', installer, full])
    let patch = null
    if (base) {
      patch = join(out, `${stem}.patch.zst`)
      run(tool, ['make-patch', base, installer, patch])
    }
    // The uncompressed installer is not published; its signature is written
    // into delta.json, where the manifest reads it.
    const installerSignature = sign(installer, version)
    sign(full, version)
    if (patch) sign(patch, version)

    const record = deltaRecord({ version, stem, from, installerSignature, hasPatch: Boolean(patch) })
    writeFileSync(join(out, 'delta.json'), `${JSON.stringify(record, null, 2)}\n`)
    process.stdout.write(`wrote the delta for ${version} to ${out}\n`)
    return
  }
  console.error('usage: node scripts/delta.mjs uncompressed <config-in> <config-out>')
  console.error('       node scripts/delta.mjs make --tool <delta-tool> --stem <name> --version <v> --installer <exe> --out <dir> [--base <exe> --from <v>]')
  process.exit(2)
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2))

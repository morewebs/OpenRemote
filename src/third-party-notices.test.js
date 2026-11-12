// The in-app updater's crates ship in the Windows installers, so the
// notices name each one at the version the shell's lock pins.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8')

function locked(lock, name) {
  const at = lock.indexOf(`\nname = "${name}"\nversion = "`)
  assert.ok(at >= 0, `${name} is in src-tauri/Cargo.lock`)
  return lock.slice(at).match(/version = "([^"]+)"/)[1]
}

test("the updater's crates are credited at the shell's locked versions", () => {
  const notices = read('../THIRD-PARTY_NOTICES.md').replace(/\r\n/g, '\n')
  const lock = read('../src-tauri/Cargo.lock').replace(/\r\n/g, '\n')
  const windows = notices.slice(notices.indexOf('## The Rust crates: the shell, Windows only'), notices.indexOf('## The Rust crates: the shell, Android only'))
  for (const name of ['tauri-plugin-updater', 'minisign-verify', 'native-tls', 'schannel', 'tokio-native-tls', 'hyper-tls', 'tempfile']) {
    assert.ok(windows.includes(`\n- ${name} ${locked(lock, name)} - `), `${name} ${locked(lock, name)} under Windows only`)
  }
  const both = notices.slice(notices.indexOf('## The Rust crates: the shell, Windows and Android'), notices.indexOf('## The Rust crates: the shell, Windows only'))
  for (const name of ['reqwest', 'ipnet', 'tokio-util']) {
    assert.ok(both.includes(`\n- ${name} ${locked(lock, name)} - `), `${name} under Windows and Android`)
  }
})

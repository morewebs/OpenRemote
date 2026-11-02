// The window's z-order is one setting with four owners, and nothing read them
// together: the console invokes the commands, the shell declares them, and the
// capability grants them. A name that drifts in one of those places is either
// a toggle that silently does nothing (Tauri refuses the invoke) or a build
// that fails the ACL check - both of which look like "the switch is broken".

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('the console, the shell and the capability agree on the two commands', () => {
  const view = read('./SettingsView.jsx')
  assert.match(view, /invoke\('window_prefs'\)/)
  assert.match(view, /invoke\('set_window_prefs', \{ keepAbove: wanted \}\)/)

  const build = read('../src-tauri/build.rs')
  const declared = [...build.matchAll(/"([a-z_]+)"/g)].map((match) => match[1])
  for (const command of ['window_prefs', 'set_window_prefs']) {
    assert.ok(declared.includes(command), `${command} is declared to Tauri`)
  }

  const caps = JSON.parse(read('../src-tauri/capabilities/default.json'))
  const granted = new Set(caps.permissions.filter((permission) => permission.startsWith('allow-')))
  for (const permission of ['allow-window-prefs', 'allow-set-window-prefs']) {
    assert.ok(granted.has(permission), `${permission} is granted to the console's webview`)
  }
})

test('the row is drawn, and searchable, where it says it is', () => {
  const view = read('./SettingsView.jsx')
  // One row, marked so a search result can land on it.
  assert.match(view, /data-stg="startup-above"/)
  assert.match(view, /checked=\{above\.keep_above\}/)
  // A row with no shell answer is absent rather than a switch that cannot act.
  assert.match(view, /if \(!above\) keys\.add\('startup-above'\)/)
  assert.match(read('./settings-search.js'), /row\('startup-above', 'startup',/)
})

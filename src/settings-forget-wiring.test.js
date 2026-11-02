// The daemon connection row (in This install since the Daemon tab folded into
// it) used to draw a disabled button whose label was a dash when nothing was
// connected. A dash in a button reads as a rendering fault, and a control that
// cannot be pressed is a dead control in the one row whose job is to say where
// the connection stands. The row now draws Forget only when there is a
// connection to forget, and the detail line under the name carries the reason
// while it is absent. This pins both halves, because the row is the first thing
// a user sees when the daemon will not answer.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = readFileSync(new URL('./SettingsView.jsx', import.meta.url), 'utf8')
// The connection row alone: from its own data attribute to the Connect by
// address row that follows it.
const row = source.slice(
  source.indexOf('data-stg="daemon-connection"'),
  source.indexOf('data-stg="daemon-connect"'),
)

test('the Forget row is the row that was read', () => {
  assert.notEqual(row.length, 0, 'the daemon connection row is gone')
  assert.match(row, /Forget/)
})

test('nothing connected draws no dash and no dead control', () => {
  // The dash label is what read as broken.
  assert.doesNotMatch(row, /'Forget' : '-'/)
  assert.doesNotMatch(row, /['"]-['"]/)
  // And the button is not rendered disabled: it is not rendered at all.
  assert.doesNotMatch(row, /disabled=/)
  assert.match(
    row,
    /\) : connection\.state === 'connected' \? \(\s*<button/,
    'the Forget button sits behind the connected state',
  )
  assert.match(row, /: null\}/, 'and the other arm renders nothing')
})

test('the reason stays in the row while the control is absent', () => {
  // The detail line, not a tooltip: this is the half that tells a user why
  // there is nothing to forget and what to do instead.
  assert.match(
    row,
    // The detail line says what is forgotten once the confirm is open, and
    // keeps the token sentence otherwise; both live in the row, not a tooltip.
    /connection\.state === 'connected'\s*\?\s*confirmDisconnect/,
  )
  assert.match(row, /Bearer token accepted\./)
  assert.match(row, /The saved address and token are forgotten; your chats, plugins and settings stay\./)
  assert.match(row, /className="pn-detail"/)
})

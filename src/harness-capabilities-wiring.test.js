// The Apps manager's capability line, wired: the daemon's honors object is
// printed per row, with the words it does not honor dimmed rather than
// hidden. Source text, like the repo's other wiring checks.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('each Apps row states what the app honors', () => {
  const view = read('HarnessManagerModal.jsx')
  assert.match(view, /\{row\.caps && \(/)
  assert.match(view, /dv-hm-cap/)
  assert.match(view, /cap\.on/)
})

test('the capability words come from the row model, not a table in the view', () => {
  const manager = read('harness-manager.js')
  assert.match(manager, /export function capabilityWords\(/)
  assert.match(manager, /caps: capabilityWords\(h\)/)
})

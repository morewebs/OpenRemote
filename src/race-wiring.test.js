// Race's wiring, read off the three files that have to agree: the palette offers
// the row and says why when there is no room, the shell mounts the modal and
// applies race.js's plan to the layout it owns, and the modal cannot be started
// twice. The rules themselves are pinned in race.test.js; this is the wiring.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const read = (name) => readFileSync(new URL(`./${name}`, import.meta.url), 'utf8')
const palette = read('CommandPalette.jsx')
const app = read('App.jsx')
const modal = read('RaceModal.jsx')

test('the palette offers the race and says why when there is no room', () => {
  assert.ok(
    palette.includes("name: 'Race a prompt across apps'"),
    'Ctrl+K has the action',
  )
  assert.ok(palette.includes('run: { race: true }'), 'and it is its own kind of row')
  assert.ok(palette.includes('else if (r.race) onRace?.()'), 'choosing it asks the shell to open the modal')
  assert.ok(palette.includes("sub: 'No free pane'"), 'a window with no room says so in the row')
})

test('the shell mounts the modal and lays the chats out through the plan', () => {
  assert.ok(app.includes('<RaceModal'), 'the modal is mounted by the shell')
  assert.ok(app.includes('free={freePanes(layout)}'), 'with the real pane count')
  assert.ok(
    app.includes('raceLayout(layout, ids)'),
    'and the layout comes from race.js, not from a second split implementation',
  )
})

test('the modal cannot be started twice, and closes through the family hook', () => {
  assert.ok(modal.includes('useDialog(panelRef'), 'the dialog plumbing is the shared one')
  assert.ok(modal.includes('if (busy || !plan.canStart) return'), 'a second press does nothing')
  assert.ok(
    modal.includes('disabled={busy || !plan.canStart}'),
    'and the button is off while the chats are being created',
  )
  assert.ok(modal.includes('useBack(!busy, onClose)'), 'Escape cannot abandon a start in flight')
  assert.ok(modal.includes('role="status"'), 'the creating sentence is announced')
})

test('a partial failure leaves only the harnesses that did not start switched on', () => {
  // Start must not be live over panes that are already open: the retry races
  // the harnesses that produced no chat, through the rule in race.js, so the
  // ones that worked cannot be raced twice.
  assert.ok(modal.includes('raceRetry(current, raced)'), 'the filter is the module\'s rule')
  assert.ok(modal.includes('raced.push(call.harness)'), 'and it is fed the harnesses that opened')
  assert.ok(
    modal.includes('setChosen((current) => raceRetry(current, raced))'),
    'a retry therefore starts from what is missing, not from the whole pick',
  )
  assert.ok(modal.includes('setBusy(false)'), 'the modal is usable again after the failure')
})

test('the failure sentence says the opened chats are behind the dialog', () => {
  assert.ok(
    modal.includes('behind this dialog'),
    'a person is told where the chats that opened are, so closing the dialog shows them',
  )
  assert.ok(
    modal.includes('stay switched on'),
    'and that the harnesses that did not start are still selected for the retry',
  )
})

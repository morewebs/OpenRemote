// Named layouts' wiring: the palette's two rows and its page, and the shell's
// seam. The rules are pinned in layouts.test.js; this proves the app calls them
// - saving is offered only with a shape, restoring goes through restoreShape
// with the window's real room, deleting asks once, and the shell applies the
// tree and navigates the primary pane.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const read = (name) => readFileSync(new URL(`./${name}`, import.meta.url), 'utf8')
const palette = read('CommandPalette.jsx')
const app = read('App.jsx')

test('saving is offered only when there is a shape to save', () => {
  assert.ok(palette.includes("name: 'Save this layout…'"), 'the row exists')
  assert.ok(palette.includes('panes > 1'), 'and only above one pane')
  assert.ok(palette.includes('onSaveLayout?.()'), 'choosing it asks the shell for the dialog')
})

test('restoring goes through the rules, with the window\'s own room', () => {
  assert.ok(palette.includes('restoreShape(r.restoreLayout, { currentChat, capacity: paneCapacity })'), 'the plan is the module\'s')
  assert.ok(palette.includes('capacityLine(plan.refusal)'), 'and a window that cannot hold it says why')
  assert.ok(palette.includes('onRestoreLayout?.(plan)'), 'only a checked plan reaches the shell')
})

test('deleting a layout asks once, through the shared armed-row rules', () => {
  assert.ok(palette.includes("from './row-confirm.js'"), 'the same rules as every other row')
  assert.ok(palette.includes('armRow(current, r.layoutArm)'), 'the first press arms')
  assert.ok(palette.includes('rowIsArmed(armedLayout, entry.id)'), 'one row at a time')
  assert.ok(palette.includes('withoutLayout(savedLayouts, r.layoutDelete)'), 'and the module removes it')
})

test('the shell gives the palette its room and applies the plan', () => {
  assert.ok(app.includes('panes={paneCount(layout)}'), 'the palette knows how many panes there are')
  assert.ok(app.includes('paneCapacity={isMobile ? 1 : MAX_PANES}'), 'and how many this window can hold')
  assert.ok(app.includes('<SaveLayoutModal'), 'the dialog is mounted by the shell')
  // A restore replaces whatever panes are open, so the shell asks first when
  // something would be lost and fits the shape to the window it lands in.
  assert.ok(app.includes('restoreConfirm(plan, layout)'), 'the shell asks before it replaces panes')
  assert.ok(app.includes('<RestoreLayoutConfirm'), 'and the question is the modal family')
  assert.ok(app.includes('if (!fitsWorkspace(plan.layout))'), 'the shape is checked against actual workspace width and height')
  assert.ok(app.includes('isChatView(plan.primaryRole)') && app.includes("plan.primaryRole === 'new'"),
    'and the primary pane navigates to what the shape said it was')
})

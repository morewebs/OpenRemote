// The picker's opening spot: the pointer when a mouse produced one, the
// trigger that has focus when a keyboard did. The failure this pins was
// visible on every picker in the console: tab to a chip, press Enter, and the
// list appeared in the window's corner because a synthesized click reports
// 0,0.

import assert from 'node:assert/strict'
import test from 'node:test'
import { isRealPoint, resolvePickerSpot, spotAtBox, spotBelow } from './picker-anchor.js'

test('a pointer position is kept exactly as the caller gave it', () => {
  const anchor = { left: 314, top: 208 }
  assert.equal(isRealPoint(anchor), true)
  assert.deepEqual(resolvePickerSpot(anchor, { left: 0, top: 0, width: 90, height: 28 }), anchor)
})

test('a synthesized click reports 0,0 and hangs off the trigger instead', () => {
  // What Enter on a focused button sends: no coordinates at all.
  const trigger = { left: 120, top: 400, width: 96, height: 28, bottom: 428 }
  assert.equal(isRealPoint({ left: 0, top: 0 }), false)
  assert.deepEqual(resolvePickerSpot({ left: 0, top: 0 }, trigger), { left: 120, top: 434 })
})

test('a missing anchor with no trigger falls back to the window corner', () => {
  assert.deepEqual(resolvePickerSpot(null, null), { left: 8, top: 8 })
  // A detached or hidden trigger has no box to hang off.
  assert.equal(spotBelow({ left: 0, top: 0, width: 0, height: 0 }), null)
  assert.deepEqual(resolvePickerSpot(undefined, { left: 5, top: 5, width: 0, height: 30 }), {
    left: 8,
    top: 8,
  })
})

test('the model menu sits on the left edge of the composer box', () => {
  assert.deepEqual(spotAtBox({ left: 200, top: 180, width: 620, height: 140, bottom: 320 }), {
    left: 200,
    top: 320,
  })
  assert.equal(spotAtBox({ left: 0, top: 0, width: 0, height: 40 }), null)
})

test('a malformed anchor is not a point', () => {
  assert.equal(isRealPoint({ left: Number.NaN, top: 4 }), false)
  assert.equal(isRealPoint({}), false)
  assert.equal(isRealPoint(null), false)
})

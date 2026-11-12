// The right-click menu's rules, read without a renderer: which rows the
// keys land on, how the arrows walk them, where a submenu opens, and the
// component's wiring of a submenu row (source pins).

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { SUBMENU_LINGER_MS, menuActions, stepCursor, submenuPlace } from './context-menu.js'

const noop = () => {}

test('the keys land on actions and on rows that open a list, never on the rest', () => {
  const items = [
    { header: 'Chat' },
    { label: 'Open', onSelect: noop },
    { divider: true },
    { label: 'Move to project', submenu: [{ label: 'No project', onSelect: noop }] },
    { label: 'Off', onSelect: noop, disabled: true },
    { label: 'A note', disabled: true },
  ]
  assert.deepEqual(
    menuActions(items).map(({ i }) => i),
    [1, 3],
  )
  assert.deepEqual(menuActions([]), [])
})

test('the arrows wrap, Home and End go to the ends, other keys are not steps', () => {
  const list = [{ i: 1 }, { i: 3 }, { i: 5 }]
  assert.equal(stepCursor(list, 0, 'ArrowDown'), 3)
  assert.equal(stepCursor(list, 2, 'ArrowDown'), 1)
  assert.equal(stepCursor(list, 0, 'ArrowUp'), 5)
  // Nothing under the cursor yet (a submenu opened by the pointer): up is the last.
  assert.equal(stepCursor(list, -1, 'ArrowUp'), 5)
  assert.equal(stepCursor(list, -1, 'ArrowDown'), 1)
  assert.equal(stepCursor(list, 1, 'Home'), 1)
  assert.equal(stepCursor(list, 1, 'End'), 5)
  assert.equal(stepCursor(list, 1, 'ArrowRight'), null)
  assert.equal(stepCursor([], 0, 'ArrowDown'), null)
})

test('a submenu opens beside its row: right where there is room, else left, else over the menu', () => {
  const rect = (left, top, width, height) => ({ left, top, right: left + width, bottom: top + height, width, height })
  const row = rect(100, 200, 180, 28)
  const menu = rect(100, 150, 180, 300)
  const list = { width: 200, height: 120 }
  // Room on the right.
  assert.deepEqual(submenuPlace({ menu, row, list, vw: 1200, vh: 900 }), { left: 284, top: 193 })
  // The right edge is near: the left side.
  const nearRight = rect(900, 150, 180, 300)
  assert.deepEqual(
    submenuPlace({ menu: nearRight, row: rect(900, 200, 180, 28), list, vw: 1200, vh: 900 }),
    { left: 696, top: 193 },
  )
  // A phone: room on neither side, so it opens over the menu, under its row.
  const phone = submenuPlace({ menu: rect(8, 150, 340, 300), row: rect(8, 200, 340, 28), list, vw: 375, vh: 800 })
  assert.deepEqual(phone, { left: 24, top: 230 })
  // Always inside the window, top and bottom.
  assert.equal(submenuPlace({ menu, row: rect(100, 860, 180, 28), list, vw: 1200, vh: 900 }).top, 772)
  assert.ok(SUBMENU_LINGER_MS > 0 && SUBMENU_LINGER_MS <= 400, 'long enough to cross rows, short enough to feel gone')
})

test('the menu draws a submenu row as one, and keeps the keyboard hand-back', () => {
  const source = readFileSync(new URL('./ContextMenu.jsx', import.meta.url), 'utf8')
  assert.match(source, /aria-haspopup=\{item\.submenu \? 'menu' : undefined\}/)
  assert.match(source, /aria-expanded=\{item\.submenu \? open === i : undefined\}/)
  assert.match(source, /e\.key === 'ArrowRight' && items\[cursor\]\?\.submenu/)
  assert.match(source, /e\.key === 'Escape' \|\| e\.key === 'ArrowLeft'/)
  // The opener gets the keyboard back when the menu goes, as before submenus.
  assert.match(source, /captureOpener\(/)
  assert.match(source, /focusOpener\(opener\)/)
  // Android's back closes it.
  assert.match(source, /useBack\(true, onClose\)/)
  // A long submenu scrolls its own list rather than closing the menu.
  assert.match(source, /if \(!subRef\.current\?\.contains\(e\.target\)\) close\.current\(\)/)
})

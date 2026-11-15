// The phone drawer keeps the page and the sidebar as two siblings: the sheet
// (the whole page, sliding right when the drawer opens) and the sidebar,
// fixed at the left edge. On Android a tap on the sidebar was answered by the
// sheet in its composited pre-animation position, so the first press looked
// dead and the second landed. The sidebar is the thing the person is
// touching after they open the drawer, so it stands above the sheet in the
// stack, and it stays hidden (out of the tap and focus paths) until the
// drawer opens.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const css = readFileSync(new URL('./sidebar.css', import.meta.url), 'utf8')

function blockFor(selector) {
  // Anchor to the start of the line: a selector can sit inside a
  // comma-separated list under a different prefix (the reduced-motion
  // block names the same suffixes), and the first raw substring would
  // land there instead of on the rule itself.
  const at = css.indexOf('\n' + selector)
  assert.notEqual(at, -1, `${selector} has a rule`)
  const open = css.indexOf('{', at)
  const close = css.indexOf('}', open)
  return css.slice(open + 1, close)
}

function zIndexOf(rule) {
  const match = rule.match(/z-index:\s*(\d+)/)
  assert.ok(match, 'the rule says its stack level')
  return Number(match[1])
}

test('the mobile sidebar stands above the sheet it reveals', () => {
  const sidebar = blockFor('.app-frame.is-mobile .sidebar')
  const sheet = blockFor('.app-frame.is-mobile .app-sheet')
  assert.ok(
    zIndexOf(sidebar) > zIndexOf(sheet),
    `the sidebar (z ${zIndexOf(sidebar)}) must outrank the sheet (z ${zIndexOf(sheet)})`,
  )
})

test('a closed drawer hides the sidebar from the tap and focus paths', () => {
  const sidebar = blockFor('.app-frame.is-mobile .sidebar')
  assert.match(
    sidebar,
    /visibility:\s*hidden/,
    'the sidebar is hidden until the drawer opens, not just below an opaque page',
  )
})

test('opening the drawer shows the sidebar at once', () => {
  const open = blockFor('.app-frame.is-mobile.drawer-open .sidebar')
  assert.match(open, /visibility:\s*visible/, 'the row a person taps is the sidebar itself')
})

test('the sidebar leaves after the sheet has slid back', () => {
  const open = blockFor('.app-frame.is-mobile.drawer-open .sidebar')
  const closed = blockFor('.app-frame.is-mobile .sidebar')
  assert.match(open, /transition:\s*visibility 0s 0s/, 'it is there the moment the drawer starts opening')
  assert.match(closed, /transition:\s*visibility 0s 0\.32s/, 'it waits until the sheet has covered it again')
})

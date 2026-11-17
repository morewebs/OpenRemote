// The pane rules are pure and tested in split.test.js; the App wiring that
// uses them is a component, so this reads its source for the seams that would
// quietly stop working in a refactor: the drop moves instead of copying, the
// cap and the phone share one decision, a beat repairs a dead pane, and a
// phone never restores a stored multi-pane layout.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = readFileSync(new URL('./App.jsx', import.meta.url), 'utf8')

const between = (from, to) => {
  const start = source.indexOf(from)
  const end = source.indexOf(to, start + from.length)
  assert.ok(start >= 0 && end > start, `${from} .. ${to} not found in App.jsx`)
  return source.slice(start, end)
}

test('a drop moves the existing pane identity, including the navigation pane', () => {
  const drop = between('const dropInPane', '// A chat asks to show another beside it')
  assert.match(drop, /if \(!leaf\) return/, 'a missing destination is refused before anything runs')
  assert.match(drop, /const source = paneForView\(layout, view, id\)/, 'the current navigation view participates in the lookup')
  assert.match(drop, /dockPane\(layout, source\.id, leafId, place\.zone\)/, 'the drop moves the actual pane rather than replacing its view')
  assert.match(drop, /place\.zone === 'center' \|\| source\.id === leafId\) setLayout\(l => focusLeaf\(l, source\.id\)\)/, 'a center drop focuses the pane already showing the chat')
  assert.ok(drop.indexOf('if (source)') < drop.indexOf('!panesCanSplit'), 'moving existing panes remains available at capacity')
  assert.match(drop, /!panesCanSplit/, 'and reads the split decision the tint reads')
  assert.match(drop, /layoutFits\(candidate, bounds\.width, bounds\.height\)/, 'edge drops use the measured workspace')
})

test('the cap and the phone are one decision, and the pane draws from it', () => {
  assert.match(source, /const panesCanSplit = !isMobile && paneCount\(layout\) < MAX_PANES/)
  assert.match(source, /canSplit=\{panesCanSplit\}/, 'the tint and the drop cannot disagree')
})

test('a beat repairs a pane whose chat is gone', () => {
  const effect = between('const seen = seenIdsRef.current', 'const view = state.history[state.hIndex]')
  assert.match(effect, /setLayout\(\(current\) => reconcilePanes\(current, gone\)\)/)
})

test('a phone never restores a stored multi-pane layout', () => {
  const load = between('function loadLayout()', 'const isChatView')
  assert.match(load, /if \(isMobile\) return newLayout\(\)/, 'a phone starts on one pane')
  assert.match(load, /reconcilePanes\(/, 'and a restored pane holding a static page is repaired before it is drawn')
})

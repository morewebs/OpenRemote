// The drag's rule lives in split.js and is tested there (one commit per
// gesture, paint on every move). This reads the pane component for the half
// a unit test cannot see: that the move handler paints and does not commit,
// and that setLayout appears exactly once in the gesture.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = readFileSync(new URL('./SplitArea.jsx', import.meta.url), 'utf8')

const startDrag = source.slice(
  source.indexOf('const startResize'),
  source.indexOf('const startPaneDrag'),
)

test('a divider drag commits the layout once, on release', () => {
  assert.equal(
    (startDrag.match(/setLayout\(/g) ?? []).length,
    1,
    'a second commit in the gesture is the pointer-rate re-render this fix removed',
  )
  assert.match(startDrag, /if \(commit && delta !== 0\) setLayout\(painted\)/, 'and only on release after the strip actually moved')
  assert.match(startDrag, /painted = resizeWorkspaceDivider\(base, divider, delta, box\.width, box\.height\)/, 'each preview uses the whole delta and respects nested pane floors')
  assert.match(startDrag, /const up = ev =>/)
  assert.match(startDrag, /workspaceGestureValid\(base, latest\.current, ev\.pointerId, pointerId\)/, 'stale gestures cannot overwrite new work')
  assert.match(startDrag, /const cancel = ev =>/)
  assert.match(startDrag, /else paint\(latest\.current\)/, 'cancellation rolls back the preview')
  assert.match(startDrag, /addEventListener\('pointercancel', cancel\)/)
  assert.match(startDrag, /addEventListener\('lostpointercapture', cancel\)/)
})

test('while the pointer moves, the strip is repainted without a commit', () => {
  const move = startDrag.slice(startDrag.indexOf('const move'), startDrag.indexOf('const finish'))
  assert.match(move, /paint\(painted\)/)
  assert.doesNotMatch(move, /setLayout\(/)
  const paint = source.slice(source.indexOf('const paint ='), source.indexOf('const startResize'))
  assert.match(paint, /workspaceGeometry\(next\)/)
  assert.match(paint, /Object\.assign\(el\.style, position\(r\)\)/, 'normalized rectangles repaint existing pane elements')
  assert.match(source, /ordered\.map\(node => <Pane key=\{node\.id\}/, 'pane siblings retain their identities when the tree changes')
})

const over = source.slice(source.indexOf('const over ='), source.indexOf('const leave ='))

test('a drag that carries no chat never tints a pane', () => {
  const gate = over.indexOf('acceptsChatDrop(e.dataTransfer.types)')
  const prevent = over.indexOf('e.preventDefault()')
  assert.ok(gate >= 0, 'the hover path asks whether the drag carries a chat')
  assert.ok(prevent > gate, 'and refuses before it prevents the default, so a file drag stays the browser\'s')
})

test('a drag that ends anywhere clears the tint it drew', () => {
  const pane = source.slice(source.indexOf('function Pane('), source.indexOf('export default function SplitArea'))
  assert.match(source, /const clear = \(\) => setPreview\(null\)/)
  assert.match(source, /window\.addEventListener\('dragend', clear\)/)
  assert.match(source, /window\.addEventListener\('drop', clear\)/)
  assert.match(source, /removeEventListener\('dragend', clear\)/)
  assert.match(source, /removeEventListener\('drop', clear\)/)
  // The pointer leaving the window is a leave too (relatedTarget null).
  assert.match(pane, /if \(e\.relatedTarget && e\.currentTarget\.contains\(e\.relatedTarget\)\) return/)
})

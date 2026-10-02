import assert from 'node:assert/strict'
import test from 'node:test'
import { editRows, isFileEdit, writeRows } from './diff.js'

test('an edit pairs its lines: same prefix and suffix, del left, add right', () => {
  const rows = editRows(
    'const a = 1\nconst b = 2\nconst c = 3\nconst d = 4',
    'const a = 1\nconst b = 20\nconst c = 30\nconst d = 4',
  )
  assert.equal(rows.length, 4)
  assert.deepEqual(rows[0].left, { text: 'const a = 1', kind: 'same' })
  assert.deepEqual(rows[0].right, { text: 'const a = 1', kind: 'same' })
  assert.deepEqual(rows[1].left, { text: 'const b = 2', kind: 'del' })
  assert.deepEqual(rows[1].right, { text: 'const b = 20', kind: 'add' })
  assert.deepEqual(rows[2].left, { text: 'const c = 3', kind: 'del' })
  assert.deepEqual(rows[2].right, { text: 'const c = 30', kind: 'add' })
  assert.deepEqual(rows[3].left, { text: 'const d = 4', kind: 'same' })
  assert.deepEqual(rows[3].right, { text: 'const d = 4', kind: 'same' })
})

test('a pure insertion has empty-left add rows; a pure deletion empty-right del rows', () => {
  const inserted = editRows('one\nthree', 'one\ntwo\nthree')
  assert.equal(inserted.length, 3)
  assert.equal(inserted[1].left.kind, null)
  assert.equal(inserted[1].right.text, 'two')
  assert.equal(inserted[1].right.kind, 'add')

  const deleted = editRows('one\ntwo\nthree', 'one\nthree')
  assert.equal(deleted.length, 3)
  assert.equal(deleted[1].left.text, 'two')
  assert.equal(deleted[1].left.kind, 'del')
  assert.equal(deleted[1].right.kind, null)
})

test('a write is all-green — every line an add on the right', () => {
  const rows = writeRows('# Notes\n\n- first')
  assert.equal(rows.length, 3)
  assert.deepEqual(
    rows.map((r) => r.right.kind),
    ['add', 'add', 'add'],
  )
  assert.ok(rows.every((r) => r.left.kind === null))
})

test('the predicate knows the edit shapes from both wires', () => {
  assert.equal(
    isFileEdit({ file_path: 'src/a.js', old_string: 'x', new_string: 'y' }),
    true,
  )
  assert.equal(isFileEdit({ file_path: 'src/a.js', content: 'new file' }), true)
  assert.equal(isFileEdit({ file_path: 'src/a.js' }), false)
  assert.equal(isFileEdit({ command: 'npm test' }), false)
  assert.equal(isFileEdit(null), false)
})

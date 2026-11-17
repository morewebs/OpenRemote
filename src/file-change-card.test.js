import test from 'node:test'
import assert from 'node:assert/strict'
import { fileChangeCards } from './file-change-card.js'

test('Claude and Pi writes show numbered content and accurate counts without a phantom final line', () => {
  for (const pathField of ['file_path', 'path']) {
    const [file] = fileChangeCards({ name: 'write', input: { [pathField]: 'hello.txt', content: '# Hello\nDemo\n' }, result: 'Created file successfully: hello.txt' })
    assert.equal(file.label, 'Created')
    assert.equal(file.added, 2)
    assert.equal(file.removed, 0)
    assert.deepEqual(file.lines.map(l => [l.number, l.text]), [[1, '# Hello'], [2, 'Demo']])
  }
})

test('edits and empty replacements retain removals, while empty new files have zero lines', () => {
  const [file] = fileChangeCards({ name: 'Edit', input: { file_path: 'a.js', old_string: 'one\ntwo\n', new_string: 'one\nthree\n' }, result: 'ok' })
  assert.equal(file.label, 'Edited')
  assert.equal(file.added, 1)
  assert.equal(file.removed, 1)
  assert.equal(file.excerpt, true)
  assert.deepEqual(file.lines.map(l => l.kind), ['same', 'del', 'add'])
  const [removed] = fileChangeCards({ input: { path: 'a.js', oldText: 'old', newText: '' }, result: 'ok' })
  assert.equal(removed.removed, 1)
  assert.equal(removed.added, 0)
  assert.equal(fileChangeCards({ input: { file_path: 'empty.txt', content: '' }, result: 'Created' })[0].lines.length, 0)
})

test('Codex fileChange supports multiple files and hunk line numbers', () => {
  const files = fileChangeCards({ name: 'fileChange', input: { changes: [
    { path: 'new.rs', kind: { type: 'add' }, diff: '+fn main() {}\n' },
    { path: 'old.rs', kind: { type: 'update' }, diff: '@@ -20,1 +20,1 @@\n-old\n+new\n' },
  ] }, result: 'completed' })
  assert.equal(files.length, 2)
  assert.equal(files[0].label, 'Created')
  assert.equal(files[1].lines[1].number, 20)
  assert.equal(files[1].removed, 1)
  const completed = fileChangeCards({ name: 'fileChange', input: { changes: [] }, result: JSON.stringify({ changes: [{ path: 'final.rs', kind: { type: 'add' }, diff: '+final\n' }] }) })
  assert.equal(completed[0].name, 'final.rs', 'the completed event carries the final diff')
})

test('failed and pending writes do not claim successful creation or expose unsupported tools', () => {
  const input = { file_path: 'hello.txt', content: '<script>unsafe</script>' }
  assert.equal(fileChangeCards({ input, state: 'failed', result: 'Permission denied' })[0].label, 'Write failed')
  assert.equal(fileChangeCards({ input, result: null })[0].label, 'Writing')
  assert.equal(fileChangeCards({ input, result: 'ok' })[0].label, 'Wrote')
  assert.deepEqual(fileChangeCards({ name: 'read', input: { path: 'a.js' }, result: 'content' }), [])
  assert.deepEqual(fileChangeCards({ input: { command: 'echo hello' } }), [])
})

test('patch content beginning with diff header characters is retained inside a hunk', () => {
  const [file] = fileChangeCards({ name: 'fileChange', input: { changes: [{ path: 'flags.txt', kind: { type: 'update' }, diff: '--- a/flags.txt\n+++ b/flags.txt\n@@ -1 +1 @@\n---old\n+++new\n' }] }, result: 'completed' })
  assert.deepEqual(file.lines.map(line => line.text), ['--old', '++new'])
  assert.equal(file.added, 1)
  assert.equal(file.removed, 1)
})

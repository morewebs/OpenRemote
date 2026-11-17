import { editRows } from './diff.js'

const linesOf = text => text === '' ? [] : text.replace(/\r\n/g, '\n').replace(/\n$/, '').split('\n')

function patchLines(diff) {
  let before = 1, after = 1, inHunk = false
  const lines = []
  for (const text of linesOf(diff)) {
    const hunk = text.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/)
    if (hunk) { before = Number(hunk[1]); after = Number(hunk[2]); inHunk = true; continue }
    if (!inHunk && /^(--- |\+\+\+ |diff |index |\\|\*\*\*)/.test(text)) continue
    if (text.startsWith('\\')) continue
    if (text.startsWith('+')) lines.push({ kind: 'add', number: after++, text: text.slice(1) })
    else if (text.startsWith('-')) lines.push({ kind: 'del', number: before++, text: text.slice(1) })
    else if (text.startsWith(' ')) {
      lines.push({ kind: 'same', number: after++, text: text.slice(1) }); before++
    }
  }
  return lines
}

function editLines(oldText, newText) {
  if (!oldText) return linesOf(newText).map((text, i) => ({ kind: 'add', number: i + 1, text }))
  if (!newText) return linesOf(oldText).map((text, i) => ({ kind: 'del', number: i + 1, text }))
  let before = 1, after = 1
  const lines = []
  for (const row of editRows(linesOf(oldText).join('\n'), linesOf(newText).join('\n'))) {
    if (row.left.kind === 'same') { lines.push({ kind: 'same', number: after++, text: row.left.text }); before++ }
    else {
      if (row.left.kind === 'del') lines.push({ kind: 'del', number: before++, text: row.left.text })
      if (row.right.kind === 'add') lines.push({ kind: 'add', number: after++, text: row.right.text })
    }
  }
  return lines
}

function card(item, path, lines, operation, excerpt = false) {
  const failed = item.state === 'failed'
  const pending = item.result == null
  const verb = operation === 'create' ? 'Created' : operation === 'delete' ? 'Deleted' : operation === 'write' ? 'Wrote' : 'Edited'
  return {
    path, name: path.split(/[\\/]/).pop() || path, lines, excerpt,
    added: lines.filter(line => line.kind === 'add').length,
    removed: lines.filter(line => line.kind === 'del').length,
    failed, pending,
    label: failed ? `${operation === 'edit' ? 'Edit' : operation === 'delete' ? 'Delete' : 'Write'} failed`
      : pending ? operation === 'edit' ? 'Editing' : operation === 'delete' ? 'Deleting' : 'Writing' : verb,
  }
}

// Normalize data already present in the tool event. A path alone is never
// enough: reads and listings must not become fabricated file changes.
export function fileChangeCards(item) {
  let input = item?.input
  if (item?.name === 'fileChange' && typeof item.result === 'string') {
    try {
      const completed = JSON.parse(item.result)
      if (Array.isArray(completed?.changes)) input = completed
    } catch { /* Older drivers may return a plain result sentence. */ }
  }
  if (!input || typeof input !== 'object') return []
  if (Array.isArray(input.changes)) {
    return input.changes.flatMap(change => {
      if (typeof change?.path !== 'string' || typeof change.diff !== 'string') return []
      const kind = typeof change.kind === 'string' ? change.kind : change.kind?.type
      return [card(item, change.path, patchLines(change.diff), kind === 'add' ? 'create' : kind === 'delete' ? 'delete' : 'edit')]
    })
  }
  const path = input.file_path ?? input.path ?? input.TargetFile
  if (typeof path !== 'string' || !path) return []
  const content = input.content ?? input.CodeContent
  if (typeof content === 'string') {
    const created = /\bcreated\b/i.test(typeof item.result === 'string' ? item.result : JSON.stringify(item.result ?? ''))
    return [card(item, path, linesOf(content).map((text, i) => ({ kind: 'add', number: i + 1, text })), created ? 'create' : 'write')]
  }
  const before = input.old_string ?? input.oldText
  const after = input.new_string ?? input.newText
  if (typeof before === 'string' && typeof after === 'string') return [card(item, path, editLines(before, after), 'edit', true)]
  return []
}

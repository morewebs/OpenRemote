// Diff shapes for the tool cards: a file edit renders as a split diff —
// old left, new right, red removals, green additions — never a JSON dump
// of the tool input. Pure functions; the card just renders the rows.

/// The kind of one side of a row: 'del' (red), 'add' (green), 'same'
/// (context, dim), or null (that side has nothing at this row).
///
/// `editRows` pairs the old and new strings line by line. The pairing is
/// a single walk from both ends — the common prefix and suffix match, and
/// the middle (where lines actually differ) pairs old→left, new→right.
/// Typical harness edits replace a contiguous block, so the walk is the
/// honest shape without a diff library.
export function editRows(oldString, newString) {
  const oldLines = String(oldString ?? '').split('\n')
  const newLines = String(newString ?? '').split('\n')
  const rows = []

  // Common prefix.
  let start = 0
  while (start < oldLines.length && start < newLines.length && oldLines[start] === newLines[start]) {
    rows.push(pair(oldLines[start], 'same'))
    start += 1
  }
  // Common suffix.
  const suffix = []
  let oldEnd = oldLines.length - 1
  let newEnd = newLines.length - 1
  while (oldEnd >= start && newEnd >= start && oldLines[oldEnd] === newLines[newEnd]) {
    suffix.unshift(pair(oldLines[oldEnd], 'same'))
    oldEnd -= 1
    newEnd -= 1
  }
  // The differing middle: old lines go left, new lines go right.
  const midLength = Math.max(oldEnd - start + 1, newEnd - start + 1)
  for (let i = 0; i < midLength; i++) {
    const oldIndex = start + i
    const newIndex = start + i
    const left = oldIndex <= oldEnd ? oldLines[oldIndex] : null
    const right = newIndex <= newEnd ? newLines[newIndex] : null
    rows.push({
      left: left == null ? { text: '', kind: null } : { text: left, kind: 'del' },
      right: right == null ? { text: '', kind: null } : { text: right, kind: 'add' },
    })
  }
  return [...rows, ...suffix]
}

/// A write is all-new from what the tool input knows: every line green.
export function writeRows(content) {
  return String(content ?? '')
    .split('\n')
    .map((text) => ({
      left: { text: '', kind: null },
      right: { text, kind: 'add' },
    }))
}

function pair(text, kind) {
  return { left: { text, kind }, right: { text, kind } }
}

/// Whether a tool input is a file edit the card can render as a diff:
/// a file path plus the edit's strings (Edit) or the whole content (Write).
export function isFileEdit(input) {
  if (!input || typeof input !== 'object') return false
  const has = (k) => typeof input[k] === 'string' && input[k].length > 0
  return (
    has('file_path') &&
    ((has('old_string') && has('new_string')) || has('content'))
  )
}

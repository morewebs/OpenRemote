// The bookmarks manager (ported from the browser branch's openremote://
// bookmarks page, 0d3211a, as a dialog in main's history family): both
// folders, searchable, reordered by drag or Alt+arrows, moved between
// folders, renamed with F2 and removed with Delete - every change through
// the store's daemon-backed updateBookmark and removeBookmark.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { inFolder, matching, otherFolder, placeAt } from './browser-bookmarks.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

const marks = [
  { id: 'a', url: 'https://a.test/', title: 'Alpha', folder: 'bar', position: 1 },
  { id: 'b', url: 'https://b.test/', title: 'Beta', folder: 'bar', position: 2 },
  { id: 'c', url: 'https://c.test/', title: 'Gamma', folder: 'bar', position: 3 },
  { id: 'd', url: 'https://docs.test/', title: '', folder: 'other', position: 1 },
]

// The folder as it would read once `mark` takes its new place.
const after = (mark, change) => inFolder(marks.map((m) => (m.id === mark.id ? { ...m, ...change } : m)), change.folder).map((m) => m.id)

test('the search matches a name or an address, in folder order, and empty is everything', () => {
  assert.deepEqual(matching(marks, 'bar', '').map((m) => m.id), ['a', 'b', 'c'])
  assert.deepEqual(matching(marks, 'bar', 'ETA').map((m) => m.id), ['b'])
  assert.deepEqual(matching(marks, 'other', 'docs').map((m) => m.id), ['d'])
  assert.deepEqual(matching(marks, 'bar', 'zzz'), [])
})

test('a drop lands where the line showed, counting the moving bookmark in place', () => {
  const [a, , c] = inFolder(marks, 'bar')
  // Alpha dropped below Beta (index 2 with Alpha still counted at 0).
  assert.deepEqual(after(a, placeAt(marks, a, 'bar', 2)), ['b', 'a', 'c'])
  // Gamma dropped at the top.
  assert.deepEqual(after(c, placeAt(marks, c, 'bar', 0)), ['c', 'a', 'b'])
  // Dropped where it already is: still there.
  assert.deepEqual(after(a, placeAt(marks, a, 'bar', 0)), ['a', 'b', 'c'])
  // Past the end of the folder.
  assert.deepEqual(after(a, placeAt(marks, a, 'bar', 3)), ['b', 'c', 'a'])
})

test('a bookmark moves between the folders, to the end or between two rows', () => {
  const b = marks[1]
  assert.equal(otherFolder(b), 'other')
  assert.equal(otherFolder(marks[3]), 'bar')
  const place = placeAt(marks, b, 'other', 1)
  assert.equal(place.folder, 'other')
  assert.deepEqual(after(b, place), ['d', 'b'])
  assert.deepEqual(after(b, placeAt(marks, b, 'other', 0)), ['b', 'd'])
  // Anything but 'other' is the bar.
  assert.equal(placeAt(marks, marks[3], 'nonsense', 0).folder, 'bar')
})

test('the manager opens from the More menu and the bar, and writes through the store', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /label: 'Bookmarks', icon: <Star size=\{14\} \/>, onSelect: \(\) => setMarksOpen\(true\)/)
  assert.match(view, /label: 'All bookmarks', icon: <Star size=\{14\} \/>, onSelect: \(\) => setMarksOpen\(true\)/)
  assert.match(view, /onUpdate=\{updateBookmark\}\s*\n\s*onRemove=\{removeBookmark\}/)
  const modal = read('./BrowserBookmarksModal.jsx')
  assert.match(modal, /useDialog\(panelRef, onClose\)/)
  assert.match(modal, /onUpdate\(mark\.id, placeAt\(marks, mark, folder, index\)\)/)
  assert.match(modal, /e\.key === 'F2'/)
  assert.match(modal, /e\.key === 'Delete'/)
  assert.match(modal, /e\.altKey && \(e\.key === 'ArrowUp' \|\| e\.key === 'ArrowDown'\)/)
  // A filtered list cannot be dragged: a drop between hidden rows lies.
  assert.match(modal, /enabled: !filter/)
  // The rename's own Esc ends the rename, not the dialog.
  assert.match(modal, /e\.key === 'Escape'\) \{\s*\n\s*\/\/[^\n]*\n\s*e\.preventDefault\(\)/)
  // Names and addresses blur in privacy mode.
  assert.match(modal, /className="bw-history-title pv"/)
})

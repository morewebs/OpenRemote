// Archive is a press away in the row's own actions menu, so an accidental
// press is easy and the only way back used to be Settings >
// Archived chats, which a first-time user does not know exists. The sidebar
// now offers the way back where the act happened: one line for eight seconds,
// replaced rather than stacked, calling the same setChatArchived the Settings
// list calls. It is not the notice stack, because that lives in the shell and
// this is the list the control lives in, and it is pinned above the scroller
// rather than inside it, because the row that went is gone from whichever
// group held it and a line that scrolls out of view is a line nobody reads.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const side = readFileSync(new URL('./Sidebar.jsx', import.meta.url), 'utf8')

test('both ways into Archive offer the way back', () => {
  // The row's actions menu and its context menu are the two presses that can
  // archive; both go through archive(), which is what raises the line. The
  // hover button the row used to carry at its edge is this menu now.
  assert.match(side, /className="chat-more"/, 'the row carries its actions button')
  assert.match(side, /items: menuItems\(row\)/, 'and that button opens the row menu')
  assert.match(side, /label: 'Archive'[^\n]*archive\(row\.id\)/, 'the row menu')
  assert.match(side, /setUndo\(row \? \{ id: row\.id, title: row\.title \} : null\)/)
})

test('one line at a time, and it expires on its own', () => {
  // A single value, so a second archive replaces the first: a history that
  // grows would teach people to ignore the line.
  assert.match(side, /const \[undo, setUndo\] = useState\(null\)/)
  assert.doesNotMatch(side, /setUndo\(\(curr/)
  assert.match(side, /const UNDO_MS = 8000/)
  assert.match(side, /setTimeout\(\(\) => setUndo\(null\), UNDO_MS\)/)
  assert.match(side, /clearTimeout\(timer\)/, 'the timer dies with the line or the sidebar')
})

test('Undo calls the same restore the Settings list calls', () => {
  assert.match(side, /setChatArchived\(undo\.id, false\)/)
  assert.match(side, /setUndo\(null\)/)
})

test('the line is in the sidebar, above the scrolled list, and says what happened', () => {
  const line = side.indexOf('className="chat-undo"')
  assert.notEqual(line, -1, 'the undo line is gone')
  // Below the actions block and above the scroller: the same place for every
  // archive, whether the row came from a project group or the flat list.
  assert.ok(line > side.indexOf('className="sb-actions"'))
  assert.ok(line < side.indexOf('<nav className="sb-list"'))
  assert.match(side, /Archived “\{undo\.title\}”/)
  assert.match(side, />\s*Undo\s*</)
  // Announced as a status, and the button names the chat it brings back.
  assert.match(side, /role="status"/)
  assert.match(side, /aria-label=\{`Undo archiving \$\{undo\.title\}`\}/)
})

// ---- the grouping cost in the sidebar ----

test('the sidebar builds the project index once and counts unread in one pass', () => {
  const src = readFileSync(new URL('./Sidebar.jsx', import.meta.url), 'utf8')
  // The index is built on a projects change, never inside the row loop.
  assert.match(src, /const folders = useMemo\(\(\) => projectIndex\(projects\), \[projects\]\)/)
  assert.match(src, /sections\(projects, rest, folders\)/)
  assert.match(src, /const unreadIn = useMemo\(\(\) => unreadProjects\(rows, unread, folders\), \[rows, unread, folders\]\)/)
  // The group badge reads the pass, not a per-group walk of its items.
  assert.match(src, /data-unread=\{unreadIn\.has\(project\.id\)/)
  assert.doesNotMatch(src, /data-unread=\{listHasUnread\(items, unread\)/)
})
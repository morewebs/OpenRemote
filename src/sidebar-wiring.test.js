// The sidebar's two claims that only its source can show.
//
// Unregistering a project used to act on the first press, on a 22px trash two
// pixels from the project's new-chat button, with no undo anywhere. It now asks
// once through the shared row-confirm rules, which the plugins, MCP and
// endpoint panes already use, so there is one "armed row" rule in the app and
// not a second one here. And registering a folder must not end in a sidebar
// that dropped it: a registered project keeps its row, its count reads zero,
// and it says it has no chats yet. The view is JSX, so this reads it rather
// than rendering it, the way the other wiring tests do.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = readFileSync(new URL('./Sidebar.jsx', import.meta.url), 'utf8')

test('the project trash asks before it unregisters', () => {
  assert.ok(
    source.includes("from './row-confirm.js'"),
    'the sidebar uses the shared armed-row rules instead of a private confirm',
  )
  assert.ok(
    source.includes('armRow(current, project.id)'),
    'the first press arms the project',
  )
  assert.ok(
    source.includes('rowIsArmed(armed, project.id)'),
    'the armed project is the one the rules name',
  )
  // The arm button is the trash; it must not also act.
  const trash = source.indexOf('className="sb-project-remove"')
  assert.ok(trash >= 0, 'the trash is still the way in')
  const end = source.indexOf('</button>', trash)
  const handler = source.slice(trash, end)
  assert.equal(
    handler.includes('removeProject('),
    false,
    'the first press must not unregister: only the confirm branch may',
  )
})

test('the confirm is the same spot, and Escape keeps the project', () => {
  const confirm = source.indexOf('className="sb-unregister"')
  assert.ok(confirm >= 0, 'the armed row swaps in the word that acts')
  const end = source.indexOf('</button>', confirm)
  const handler = source.slice(confirm, end)
  assert.ok(handler.includes('Escape'), 'Escape disarms')
  assert.ok(handler.includes('clearRow()'), 'and clears through the shared rule')
  assert.ok(handler.includes('removeProject(project.id)'), 'the second press acts')
  // The confirm takes the keyboard when it replaces the trash, so the keyboard
  // path lands on it rather than at the start of the document. The panes' armed
  // rows do the same through use-armed-row's callback ref.
  assert.ok(handler.includes('autoFocus'), 'the confirm is focused where it appears')
})

test('a registered project with no chats keeps its row and says so', () => {
  // The count is a sentence now, not a numeral: a collapsed head says what it
  // holds ("0 chats" included, which is what tells a person the project is
  // empty without expanding it), and the expanded head says nothing unless
  // something is waiting. projects.js owns the copy; the view renders parts.
  assert.ok(
    source.includes("projectLine({ chats: items.length, waiting: waitingIn.get(project.id) ?? 0 }, { collapsed: isCollapsed })"),
    'the head reads its line from the shared rule',
  )
  assert.ok(source.includes('{held && ('), 'and draws it only when there is something to say')
  assert.ok(
    source.includes("part.kind === 'waiting' ? ' is-waiting' : ''"),
    'the waiting part wears the status colour',
  )
  const empty = source.indexOf('items.length === 0')
  assert.ok(empty >= 0, 'an empty project has its own state')
  assert.ok(
    source.slice(empty, empty + 400).includes('No chats yet.'),
    'and says it in one line rather than reading as an absence',
  )
  // A project whose folder is gone is its own sentence rather than reading as empty:
  // its chats have fallen to the flat list, so "No chats yet" would be the wrong news.
  assert.ok(
    source.slice(empty, empty + 400).includes('No folder behind this project'),
    'and a folder-less project says what happened to it',
  )
})

test('the generic empty lines yield to a registered project', () => {
  assert.ok(
    source.includes('rows.length === 0 && grouped.projects.length === 0'),
    'with a project on screen, its own line is the one that speaks',
  )
  assert.ok(
    source.includes('grouped.flat.length === 0 && rows.length > 0'),
    'and no line talks about chats outside projects when there are no chats at all',
  )
})

test('the chat list is one Tab stop, and the arrows walk it', () => {
  // A Tab per row plus a Tab per row's hover archive button put the view's
  // first control 40-odd Tabs away once the list held a few dozen chats, which
  // made the console unusable from the keyboard. One row carries the stop and
  // the arrows move inside the list, the way a listbox behaves.
  assert.match(source, /const \[rovingId, setRovingId\] = useState\(null\)/, 'the roving stop has a home')
  assert.match(source, /const rovingRow = rovingId && order\.includes\(rovingId\) \? rovingId : order\[0\]/)
  assert.match(source, /data-chat-row=\{row\.id\}/, 'a row can be found again after a re-render')
  assert.match(source, /tabIndex=\{rovingRow === row\.id \? 0 : -1\}/, 'exactly one row is in the Tab order')
  assert.match(source, /moveRoving\(row\.id, to\)/, 'the arrows move between rows')
  assert.match(source, /'ArrowDown' \? 1 : e\.key === 'ArrowUp' \? -1/, 'up and down walk the list')
  // The row's actions button is a pointer shortcut: archiving from the
  // keyboard is Mod+Shift+A or the palette's row, and leaving the button in
  // the order would double the cost of every row again.
  assert.match(source, /className="chat-more"[\s\S]{0,220}tabIndex=\{-1\}/, 'the actions button stays out of the Tab order')
})
test('a row that vanishes under the keyboard hands its place to its neighbour', () => {
  // The roving stop already falls back to the first row; this is the person's
  // place. The effect watches the drawn order and acts only when the row that
  // held the keyboard has gone and focus was dropped to the body - a focus
  // that landed on another control clears the memory instead, so a list nobody
  // has touched never takes the keyboard.
  assert.ok(source.includes("const orderKey = order.join('|')"), 'one effect per order, not per render')
  assert.ok(
    source.includes('if (!held || order.includes(held)) return'),
    'a row that is still drawn is left exactly where it is',
  )
  assert.ok(
    source.includes('if (document.activeElement && document.activeElement !== document.body) {'),
    'a person who moved on is not pulled back',
  )
  assert.ok(source.includes('const at = before.indexOf(held)'), 'the index the row held')
  assert.ok(
    source.includes('order[Math.min(Math.max(at, 0), order.length - 1)]'),
    'the neighbour at the same index, clamped, or the first row',
  )
  assert.ok(
    source.includes('[data-chat-row="${CSS.escape(target)}"]'),
    'and the row that took its place receives the keyboard',
  )
  assert.ok(source.includes('}, [orderKey])'), 'keyed on the order string, not the array')
  assert.ok(source.includes('focusedRow.current = row.id'), 'the row says it holds the keyboard')
})
test('a chat row offers Copy as Markdown where the copy can work', () => {
  // The rule, the write and the two sentences are chat-markdown.js's, the
  // same module the palette's row reads, so the two ways in cannot drift.
  assert.match(source, /import \{ canCopyChat, chatMarkdown, copiedLine \} from '\.\/chat-markdown\.js'/)
  assert.match(source, /import \{ errorCopy \} from '\.\/error-copy\.js'/)
  // The item sits directly after Rename, and only where there is something to
  // write: a row that answered "nothing to copy" after a press is worse than
  // one that is not offered.
  const menu = source.slice(source.indexOf('const menuItems = (row, at = null) => {'), source.indexOf('// The Mobile tile'))
  const renameAt = menu.indexOf("label: 'Rename'")
  const copyAt = menu.indexOf("label: 'Copy as Markdown'")
  // Move to project sits between them now: both are still above the copy and
  // the copy above the split rows.
  const moveAt = menu.indexOf("label: 'Move to project'")
  assert.ok(renameAt > 0 && moveAt > renameAt && copyAt > moveAt, 'the copy follows Rename and Move to project')
  assert.match(menu, /canCopyChat\(chats\?\.\[row\.id\]\)/, 'and is gated on the module rule')
  assert.ok(copyAt < menu.indexOf("label: 'Split right'"), 'the split order stays after it')
  assert.ok(copyAt < menu.indexOf("label: 'Archive'"))
  assert.ok(copyAt < menu.indexOf("label: 'Delete chat'"))
  assert.ok(copyAt < menu.indexOf("label: 'Copy title'"))
  // The write is the window's own clipboard, and the answer is the module's
  // sentence, announced in the sidebar's transient line rather than dropped.
  assert.match(source, /navigator\.clipboard\?\.writeText\?\.\(file\.text\)/)
  assert.match(source, /write\.then\(\(\) => setCopyNote\(copiedLine\(file\)\)\)/)
  assert.match(source, /\.catch\(\(err\) => setCopyNote\(errorCopy\(err\)\)\)/)
  assert.match(source, /setCopyNote\('Nothing to copy in this chat\.'\)/)
  assert.match(source, /setCopyNote\('Could not copy: this window has no clipboard\.'\)/)
  assert.match(source, /\{copyNote && \(\s*<div className="chat-undo" role="status">/)
})
test('a row menu keeps the items it opened with', () => {
  // The copy item is gated on the chat's transcript being in this window, and
  // a transcript can arrive while the menu is open. Rebuilding the rows then
  // would insert one in the middle and shift every later index, and the menu
  // moves its highlight by index - so the target would move under the
  // keyboard. The rows are built once, when the menu opens, and kept with it.
  assert.match(source, /items: menuItems\(row, \{ x: e\.clientX, y: e\.clientY \}\)/)
  assert.match(source, /setMenu\(\{/)
  assert.match(source, /x: e\.clientX,/)
  assert.match(source, /y: e\.clientY,/)
  assert.match(source, /items: menuItems\(row, \{ x: e\.clientX, y: e\.clientY \}\),/)
  assert.match(source, /items=\{menu\.items\}/, 'the menu draws the rows it was given')
  assert.doesNotMatch(source, /menuItems\(menu\.row\)/, 'the open menu never rebuilds its rows')
})
test('a search opens the projects it matches, whatever the person folded', () => {
  // The grouped rows only draw while a project is open, so a folded project
  // would hide the one matching chat from the field the person is typing in.
  assert.ok(
    source.includes('const openedBySearch = useMemo(() => {'),
    'the search decides which projects to open',
  )
  assert.ok(source.includes('if (items.length > 0 || projectName(project).toLowerCase().includes(q)) ids.add(project.id)'))
  assert.ok(
    source.includes('const folded = (id) => !!collapsed[id] && !openedBySearch.has(id)'),
    'one rule for folded, used by the head and the keyboard order',
  )
  assert.ok(
    source.includes('if (folded(project.id)) return []'),
    'the arrows walk into a project the search opened, and skip its folded rows',
  )
  assert.equal(
    (source.match(/folded\(project\.id\)/g) ?? []).length,
    2,
    'the head and the order both ask the same question',
  )
})

test('the folded projects are remembered, and stale ids are forgotten', () => {
  assert.ok(source.includes("const COLLAPSED_KEY = 'openremote-collapsed-projects'"))
  assert.ok(source.includes('const [collapsed, setCollapsed] = useState(loadCollapsed)'))
  assert.ok(
    source.includes('if (next[id]) delete next[id]'),
    'a project that was folded open is dropped from the stored map, not stored as false',
  )
  assert.ok(source.includes('storeCollapsed(next)'), 'and the change is written at the press')
  // A stale id cannot fold a project that takes it later.
  assert.ok(source.includes('for (const id of ids) if (known.has(id)) kept[id] = true'))
  assert.ok(source.includes('storeCollapsed(kept)'))
})
test('a row whose stream is between attempts says reconnecting in its tooltip', () => {
  // The same word the pane header uses, from the one constant, read off the
  // console's per-chat mirror. No dot and no status change: a stopped
  // transcript must not read as an agent still thinking.
  assert.match(source, /import \{ RECONNECTING \} from '\.\/pane-header\.js'/)
  assert.match(source, /reconnecting \} = useConsole\(\)/)
  assert.match(source, /title=\{reconnecting\?\.\[row\.id\] \? `\$\{row\.title\} - \$\{RECONNECTING\}` : undefined\}/)
})
test('the waiting lines are asked for in one pass, and one failure does not sink the rest', () => {
  // Three waiting chats used to be three awaits in a row: the first line
  // appeared, then the next, and the stamp that started the pass re-entered
  // between rows. One Promise.all settles them together.
  const effect = source.slice(source.indexOf('const missing = missingLines(kept, needs.chats)'), source.indexOf('// The two sections: registered projects'))
  assert.ok(effect.length > 0, 'the question-lines effect is in the file')
  assert.match(effect, /const asked = await Promise\.all\(/)
  assert.match(effect, /missing\.map\(async \(row\) => \{/)
  // Each ask owns its failure: a rejected decision read becomes this chat's
  // empty line, so the batch never rejects and the others still land.
  assert.match(effect, /catch \{[\s\S]{0,220}return \{ row, line: null \}/)
  // One settle for the whole pass, after every ask is in.
  assert.equal((effect.match(/setQuestions\(/g) ?? []).length, 1, 'one state write per pass')
  assert.match(effect, /for \(const \{ row, line \} of asked\)/)
  // The rules that decide which chats ask, and how an answer is cached, are
  // the ones that were there: missingLines over the same kept cache, keyed by
  // the stamp, kept even when null so a failure is not retried every beat.
  assert.match(effect, /const missing = missingLines\(kept, needs\.chats\)/)
  assert.match(effect, /cache = rememberLine\(cache, questionKey\(row\.id, row\.updatedAt\), line\)/)
  assert.doesNotMatch(effect, /for \(const row of missing\)/, 'no serial walk')
  // And the pass still stands down when the dependency changes under it.
  assert.match(effect, /if \(cancelled\) return/)
})
test('a chat row menu offers Move to project, and a refusal keeps the daemon words', () => {
  // The row the menu carries, and the picker the app already has behind it:
  // "No project" first, the current project marked, and the route as the only
  // place a chat's project changes.
  const at = source.indexOf('const menuItems = (row, at = null) => {')
  assert.ok(at >= 0, 'the row menu takes the position the picker opens at')
  const menu = source.slice(at, at + 5000)
  assert.match(menu, /label: 'Move to project'/)
  assert.match(menu, /onSelect: \(\) => setMoving\(\{ row, at \}\)/)
  assert.match(source, /const moveTargets = \(\) => \[/)
  assert.match(source, /\{ id: 'none', name: 'No project' \}/)
  assert.match(source, /<PickerMenu[\s\S]{0,500}label="Move to project"/)
  assert.match(source, /api\.call\('POST', `\/sessions\/\$\{row\.id\}\/workspace`/)
  // A failure is the daemon's sentence, not a swallowed one.
  assert.match(source, /catch \(err\) \{\s*setCopyNote\(errorCopy\(err\)\)/)
})
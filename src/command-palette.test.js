// The palette's one way into the review surface. The palette gets no prop
// about the view, so these pin the two things that keep the row honest:
// which chat it reads (the address, the shell's own route) and that it is
// only offered when there is something to read.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the palette asks the review surface to open', () => {
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /import \{ openChanges \} from '\.\/ChangesModal\.jsx'/)
  // The row's run reaches the opener after the palette has closed, beside
  // the other destinations, rather than navigating anywhere. It names the
  // chat, so a review opened for a pane cannot land on the wrong one.
  assert.match(palette, /else if \(r\.changes\) openChanges\(r\.changes\)/)
})

test('the row is offered for the chat the address names, and only with changes', () => {
  const palette = source('CommandPalette.jsx')
  // The address is the shell's route: App writes it on every navigation, and
  // it is the only signal the palette has about the chat on screen.
  assert.match(palette, /location\.hash\.replace\(\/\^#\/, ''\)/)
  assert.match(palette, /const reviewChat = chats\?\.\[onScreen\] \?\? null/)
  // A yes or no, not a fold: the palette recomputes on every stream beat.
  assert.match(palette, /import \{ hasFileChanges \} from '\.\/changes\.js'/)
  assert.match(palette, /hasFileChanges\(reviewChat\.timeline\)/)
  assert.doesNotMatch(palette, /fileChanges\(/)
  // The row's id is the chat it reviews, so a stale row cannot open another.
  assert.match(palette, /run: \{ changes: reviewChat\.id \}/)
})

test('the row reads as one action among the others, with its own mark', () => {
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /name: 'Changes in this chat'/)
  assert.match(palette, /section: 'Actions'/)
  assert.match(palette, /<FileCode size=\{15\} \/>/)
  // It rides the empty query too: nothing else in the console reaches it yet.
  assert.match(palette, /'action:changes',/)
})

test('the palette keeps its other destinations intact', () => {
  // The new branch sits between actions and chats; the rest still resolve.
  const palette = source('CommandPalette.jsx')
  for (const branch of ['r.action', 'r.chat', 'r.project', 'r.machine']) {
    assert.match(palette, new RegExp(branch.replace('.', '\\.')), `${branch} still handled`)
  }
})

// ---- the palette's row identity ----

const palette = readFileSync(new URL('./CommandPalette.jsx', import.meta.url), 'utf8')

test('a token event does not rebuild the palette rows', () => {
  // The row list's dependencies no longer include the chat records, whose
  // every mutation used to rebuild two hundred rows, and they do not include
  // the browser's tabs either: the Browser group has its own memo below, so a
  // page event rebuilds that group and nothing else.
  assert.match(palette, /\}, \[kb, sessions, archived, projects, devices, thisDevice, capabilities, machines, raceFree, moved, panes, savedLayouts, onScreen, reviewChat, markdownChat, hasFileChanges, focusedChatId, onRenameChat, rowSetup, onNewWithSetup, rulesCanRun, automations, machinesAnswered, onWorkspaceLayout\]\)/)
  // The setup row is the one that reads a chat's own fields, and it is built in
  // a memo keyed on those fields: the record is read per render, but a token
  // that grows the timeline rebuilds nothing (chat-setup.js answers the setup).
  assert.match(palette, /\[setupFields, capabilities, projects, thisDevice\],/)
  assert.doesNotMatch(palette, /\}, \[kb, sessions, chats,/)
  assert.doesNotMatch(palette, /machinesAnswered, tabs\]\)/, 'the tabs are not a dependency of every row')
  // The two timeline-reading rows are built apart and stabilised by their own
  // key, so an unchanged answer reuses the same row objects.
  assert.match(palette, /const chatActions = useMemo\(\(\) => \{/)
  assert.match(palette, /if \(chatActionsRef\.current\.key !== chatActions\.key\) chatActionsRef\.current = chatActions/)
})

test('the browser group is wired to the store the strip already uses', () => {
  const palette = source('CommandPalette.jsx')
  // The rows come from palette-tabs.js, not from a second tab reader.
  assert.match(palette, /import \{ BROWSER_GROUP, browserTabs \} from '\.\/palette-tabs\.js'/)
  assert.match(palette, /section: BROWSER_GROUP,/)
  assert.match(palette, /run: \{ tab: tab\.key \},/)
  // The group appears only while a browser view is mounted: the store's own
  // registration says so (registerReopen, made by the view while it is on
  // screen), and reopenAvailable is not used for it because that one also
  // needs a closed tab.
  assert.match(palette, /import \{ reopenAvailable, reopenClosed, browserMounted, getState, setState, subscribe \} from '\.\/browser-store\.js'/)
  assert.match(palette, /const \[tabs, setTabs\] = useState\(\(\) => getState\(\)\.tabs\)/)
  assert.match(palette, /useEffect\(\(\) => subscribe\(\(\) => setTabs\(getState\(\)\.tabs\)\), \[\]\)/)
  // The rows have their own memo, keyed on the tab list and the checked
  // mount, the way chatActions keeps its two rows apart: a page event (a
  // title, a url, a favicon, a loading state) rebuilds this group only, and
  // the rows keep their identity until the next one.
  assert.match(palette, /const browserIsMounted = browserMounted\(\)/)
  assert.match(palette, /const tabRows = useMemo\(\s*\(\) =>\s*browserTabs\(\{ tabs, mounted: browserIsMounted \}\)/)
  assert.match(palette, /\[tabs, browserIsMounted\],\s*\)/)
  assert.doesNotMatch(palette, /const tabRows = browserTabs\(/, 'the rows are never built outside the memo')
  // Choosing a row is the strip's own switch: the store's active tab, then
  // the palette closes because the place chosen is behind it.
  assert.match(palette, /if \(r\.tab\) \{/)
  assert.match(palette, /setState\(\(\) => \(\{ active: r\.tab \}\)\)/)
  // It rides the same ranking as every other group, from the memo it owns,
  // and the section order knows its name.
  assert.match(palette, /\.\.\.everything\.ruleRows, \.\.\.tabRows\]/)
  assert.match(palette, /\}, \[page, query, everything, tabRows,/)
  assert.match(palette, /const SECTION_ORDER = \['Actions', 'Rules', 'Chats', 'Views', 'Projects', 'Machines', 'Browser'\]/)
  // And the store gained the mounted getter the group asks for.
  const store = source('browser-store.js')
  assert.match(store, /export function browserMounted\(\) \{\s*return Boolean\(reopenHandler\)/)
})
test('a capped group says how many rows it did not show', () => {
  // Fifty chats or ten rules used to lose the ninth match in silence; the
  // palette looked broken rather than narrow. The last row is the fact, not a
  // link, and Enter on it does nothing.
  assert.ok(palette.includes('const rest = forSection.length - SECTION_CAP'))
  assert.ok(palette.includes('name: `and ${rest} more`'))
  assert.ok(palette.includes("sub: 'Refine the search to narrow it'"))
  assert.ok(palette.includes('run: null,'))
  assert.ok(palette.includes('if (!row.run) return'), 'the fact row cannot be chosen')
})
test('a tie prefers a label the query begins, then a chat over an action', () => {
  const palette = source('CommandPalette.jsx')
  // The rule lives beside the ranker, where it has its own test; the palette
  // supplies the one fact it knows - which rows are chats - and nothing else.
  assert.match(palette, /import \{ ACTIONS, comboFromEvent, comboLabel, rankMatches, tieRule \} from '\.\/shortcuts\.js'/)
  assert.match(palette, /rankMatches\(query, all, \(r\) => r\.name, tieRule\(query, \(row\) => row\.section === 'Chats'\)\)/)
  // The score still ranks: the palette never re-sorts, and never touches the
  // scorer itself.
  assert.doesNotMatch(palette, /fuzzyScore/)
  assert.doesNotMatch(palette, /\.sort\(/)
})

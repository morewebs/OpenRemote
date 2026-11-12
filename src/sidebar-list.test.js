// The sidebar's pin and page rules, and the pins the view draws from them.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

// The module reads and writes localStorage at call time, not at import.
const store = new Map()
globalThis.localStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k),
  clear: () => store.clear(),
}

const {
  CHATS_PAGE,
  PROJECT_PAGE,
  loadPins,
  moreHint,
  pinnedRows,
  planList,
  storePins,
  togglePin,
} = await import('./sidebar-list.js')

const row = (id, over = {}) => ({ id, title: id, ...over })
const rows = (n, prefix = 'c') => Array.from({ length: n }, (_, i) => row(`${prefix}${i + 1}`))

test('a pin goes to the front, unpinning takes it out, and pins keep that order', () => {
  let pins = []
  pins = togglePin(pins, 'a')
  pins = togglePin(pins, 'b')
  pins = togglePin(pins, 'c')
  // Newest pin first, the order a person pinned them in.
  assert.deepEqual(pins, ['c', 'b', 'a'])
  pins = togglePin(pins, 'b')
  assert.deepEqual(pins, ['c', 'a'])
  // No id is no change: the same list comes back, so the caller can skip a write.
  assert.equal(togglePin(pins, ''), pins)
  assert.equal(togglePin(pins, null), pins)
  // The pins round-trip through this device's own key, repaired.
  storePins(pins)
  assert.deepEqual(loadPins(), ['c', 'a'])
  storePins(['a', 'a', 7, null, 'b'])
  assert.deepEqual(loadPins(), ['a', 'b'])
})

test('a pinned chat sits at the top of its own list, in pin order, once', () => {
  const items = rows(7)
  const plan = planList(items, { pins: ['c5', 'c2'] })
  assert.deepEqual(plan.pinned.map((r) => r.id), ['c5', 'c2'])
  // It is not drawn again among the paged rows.
  assert.deepEqual(plan.shown.map((r) => r.id), ['c1', 'c3', 'c4', 'c6', 'c7'])
})

test('a pin whose chat is gone is skipped, and a moved chat pins in its new list', () => {
  const listA = [row('a1'), row('a2')]
  const listB = [row('b1'), row('b2')]
  const pins = ['a2', 'deleted']
  // Deleted: no row, no empty row.
  assert.deepEqual(pinnedRows(listA, pins).map((r) => r.id), ['a2'])
  // Moved: the group's own rows decide, so the pin shows where the chat is now.
  assert.deepEqual(planList(listA, { pins }).pinned.map((r) => r.id), ['a2'])
  assert.deepEqual(planList(listB, { pins }).pinned, [])
  // The flat list has no project, and a pinned chat there is the same rule.
  assert.deepEqual(planList([row('a2'), row('x')], { pins }).pinned.map((r) => r.id), ['a2'])
})

test('a pinned chat that is also the open one is drawn once, at the top', () => {
  const plan = planList(rows(6), { pins: ['c1'], keep: (r) => r.id === 'c1' })
  assert.deepEqual(plan.pinned.map((r) => r.id), ['c1'])
  assert.equal(plan.shown.some((r) => r.id === 'c1'), false)
  assert.equal([...plan.pinned, ...plan.shown].filter((r) => r.id === 'c1').length, 1)
})

test('the page holds five per project and fifteen for the flat list, and Show more reveals the rest', () => {
  assert.equal(PROJECT_PAGE, 5)
  assert.equal(CHATS_PAGE, 15)
  const plan = planList(rows(9))
  assert.equal(plan.shown.length, PROJECT_PAGE)
  assert.equal(plan.next, 9 - PROJECT_PAGE)
  // One press shows the rest of that list, not one more page of it.
  const more = planList(rows(9), { expanded: true })
  assert.equal(more.shown.length, 9)
  assert.deepEqual(more.next, 0)
  // The flat list's own page.
  const flat = planList(rows(40), { page: CHATS_PAGE })
  assert.equal(flat.shown.length, CHATS_PAGE)
  // The open chat stays in view wherever it sits, and the count is what is
  // actually left out.
  const late = planList(rows(9), { keep: (r) => r.id === 'c9' })
  assert.equal(late.shown.length, PROJECT_PAGE + 1)
  assert.equal(late.next, 3)
})

test('the sidebar draws pins, the Show more line and the menu items from these rules', () => {
  const sidebar = readFileSync(new URL('./Sidebar.jsx', import.meta.url), 'utf8')
  const css = readFileSync(new URL('./sidebar.css', import.meta.url), 'utf8')
  assert.match(sidebar, /from '\.\/sidebar-list\.js'/)
  assert.match(sidebar, /const \[pins, setPins\] = useState\(loadPins\)|useState\(\(\) => loadPins\(\)\)/)
  assert.match(sidebar, /planList\(items, \{\s*pins/)
  assert.match(sidebar, /planList\(grouped\.flat|planFor\('flat'/)
  // The line is drawn only while something is left out, and names it.
  assert.match(sidebar, /Show \{plan\.next\} more/)
  assert.match(sidebar, /moreLine\(project\.id, plan\)/)
  assert.match(sidebar, /moreLine\('flat', flatPlan\)/)
  // The pin is in the row's own menu, and the row wears a mark.
  assert.match(sidebar, /label: pinned \? 'Unpin' : 'Pin'/)
  assert.match(sidebar, /className="chat-pin"/)
  assert.match(css, /\.chat-pin \{/)
})

// ---- what "Show N more" leaves out ----

test('the page names the rows it leaves out, so a hint can say what they are up to', () => {
  const list = rows(8)
  const plan = planList(list, { pins: ['c1'], keep: (r) => r.id === 'c8' })
  // c1 is pinned, c2..c6 the page, c8 kept: c7 alone is out of sight.
  assert.deepEqual(plan.hidden.map((r) => r.id), ['c7'])
  assert.equal(plan.next, plan.hidden.length)
  assert.deepEqual(planList(list, { expanded: true }).hidden, [])
})

test('"Show N more" hints at a failure or a running turn among what it leaves out', () => {
  // Ported from br-sidebar b941184's dot on the line, in main's words: a long
  // list never hides a failure behind the page.
  assert.deepEqual(moreHint([{ status: 'failed' }, { status: 'working' }, { status: 'idle' }, { status: 'running' }]), [
    { kind: 'failed', text: '1 failed' },
    { kind: 'running', text: '2 running' },
  ])
  assert.deepEqual(moreHint([{ status: 'starting' }]), [{ kind: 'running', text: '1 running' }])
  // Nothing to say, or an away copy's stale status: no hint at all.
  assert.equal(moreHint([{ status: 'idle' }, { status: 'stopped' }]), null)
  assert.equal(moreHint([{ status: 'failed', offline: true }]), null)
  assert.equal(moreHint([]), null)
})

test('the sidebar draws the hint, and the quiet unread step, on both Show more lines', () => {
  const sidebar = readFileSync(new URL('./Sidebar.jsx', import.meta.url), 'utf8')
  const css = readFileSync(new URL('./sidebar.css', import.meta.url), 'utf8')
  const at = sidebar.indexOf('const moreLine = (key, plan) => {')
  assert.ok(at > 0, 'one line, drawn for a project and for the flat list')
  const line = sidebar.slice(at, at + 1600)
  assert.match(line, /const hint = moreHint\(plan\.hidden\.flatMap\(withTeam\)\)/)
  assert.match(line, /data-unread=\{listHasUnread\(plan\.hidden, unread\) \? 'true' : undefined\}/)
  assert.match(line, /className="sb-more-hint"/)
  assert.match(line, /part\.kind === 'failed' \? ' is-failed' : ''/)
  assert.match(css, /\.sb-more\[data-unread='true'\]/)
  assert.match(css, /\.sb-more-hint \{/)
})

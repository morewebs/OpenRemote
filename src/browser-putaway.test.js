// Putting pages away past ten (ported from the browser branch's 0d3211a):
// each open page holds memory, so past ten the least recently shown go back
// to entries (address and title) that open again when shown. The tab on
// screen, an agent's tab and a private tab are never put away.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { LIVE_LIMIT, isDormant, putAway, toDiscard } from './browser-tabs.js'
import { adopt, getState, setState } from './browser-store.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

const pages = (n) =>
  Array.from({ length: n }, (_, i) => ({ key: `k${i}`, native: `tab-${i}`, url: `https://p${i}.test/`, seen: i }))

test('ten pages stay open; past ten the least recently shown are put away', () => {
  assert.equal(LIVE_LIMIT, 10)
  assert.deepEqual(toDiscard(pages(10), 'k0'), [])
  const tabs = pages(12)
  // k0 is shown, so the next two oldest go.
  assert.deepEqual(toDiscard(tabs, 'k0').map((t) => t.key), ['k1', 'k2'])
  // Entries (no page) don't count toward the ten.
  assert.deepEqual(toDiscard([...pages(10), { key: 'e', url: 'https://e.test/', native: null }], 'k0'), [])
})

test("an agent's tab, a private tab and the shown tab are never put away", () => {
  const tabs = pages(12)
  tabs[1].controller = { kind: 'agent', chat: 'c', title: 'Chat' }
  tabs[2].private = true
  assert.deepEqual(toDiscard(tabs, 'k3').map((t) => t.key), ['k0', 'k4'])
})

test('a page put away keeps its address and waits to open again', () => {
  const away = putAway({ key: 'a', native: 'tab-1', url: 'https://a.test/', title: 'A', loading: true, canBack: true, find: { term: 'x' } })
  assert.deepEqual(away, { key: 'a', native: null, url: 'https://a.test/', title: 'A', loading: false, canBack: false, canForward: false, find: null })
  assert.ok(isDormant(away))
})

test('the store puts the oldest away as an eleventh page lands, and marks the shown tab as seen', () => {
  const tabs = pages(10)
  tabs.push({ key: 'new', native: null, url: 'https://new.test/', loading: true })
  setState(() => ({ tabs, active: 'k5', prompts: [{ tab: 'tab-0', id: 1 }] }))
  assert.ok(getState().tabs.find((t) => t.key === 'k5').seen > 100, 'shown just now')
  adopt('new', 'tab-new')
  const live = getState().tabs.filter((t) => t.native)
  assert.equal(live.length, 10)
  const gone = getState().tabs.find((t) => t.key === 'k0')
  assert.equal(gone.native, null)
  assert.equal(gone.url, 'https://p0.test/')
  assert.deepEqual(getState().prompts, [], "its site's request goes with the page")
})

test('the view reopens a put-away tab when shown', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /!isDormant\(current\)\) return\s+openInRef\.current\(current, current\.url\)/)
  const store = read('./browser-store.js')
  assert.match(store, /patchTab\(key, \{ native, \.\.\.said \}\)\s+enforceMemory\(\)/)
})

test('the tab just left ranks as recently shown, so it is not among the first put away', async () => {
  const tick = () => new Promise((r) => setTimeout(r, 3))
  const tabs = pages(11).map((t) => ({ ...t, seen: undefined }))
  setState(() => ({ tabs, active: 'k0', prompts: [] }))
  // Visit k1..k9 briefly, then sit in k10 and leave it for k0.
  for (let i = 1; i <= 10; i += 1) {
    await tick()
    setState(() => ({ active: `k${i}` }))
  }
  await tick()
  setState(() => ({ active: 'k0' }))
  const seen = Object.fromEntries(getState().tabs.map((t) => [t.key, t.seen]))
  // k10 was left at the moment k0 was shown: newer than every brief visit.
  assert.equal(seen.k10, seen.k0)
  for (let i = 1; i <= 9; i += 1) assert.ok(seen.k10 > seen[`k${i}`], `k10 newer than k${i}`)
  // k1 was left first, so it is the oldest.
  assert.ok([2, 3, 4, 5, 6, 7, 8, 9].every((i) => seen.k1 < seen[`k${i}`]))
  assert.deepEqual(toDiscard(getState().tabs, 'k0').map((t) => t.key), ['k1'])
})

test('the first tab is stamped as shown from the start', () => {
  const store = read('./browser-store.js')
  assert.match(store, /const first = newTab\(\{ seen: Date\.now\(\) \}\)/)
})

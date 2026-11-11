// Session restore (ported from the browser branch's 0d3211a, 01aee56 and
// the "On startup" half of 015ea58): the last session's tabs come back at
// the browser's first open after a start, even when agents opened tabs
// first; only the shown one opens its page, the rest when first shown; the
// tabs are saved as they change and never include a private tab.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { isDormant, restoredTabs, sessionOf, withRestored } from './browser-tabs.js'
import { BROWSER_DEFAULTS, STARTUP_CHOICES, browserSetting } from './browser-prefs.js'
import { getState, setState, startSession } from './browser-store.js'
import { SETTINGS_ENTRIES, searchSettings } from './settings-search.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('the saved session keeps web tabs only, never a private one, and which was shown', () => {
  const tabs = [
    { key: 'a', url: 'https://a.test/', title: 'A' },
    { key: 'n', url: '' },
    { key: 'p', url: 'https://secret.test/', private: true },
    { key: 'b', url: 'http://localhost:5173/', title: 'Dev' },
    { key: 'j', url: 'javascript:alert(1)' },
  ]
  assert.deepEqual(sessionOf(tabs, 'b'), {
    tabs: [
      { url: 'https://a.test/', title: 'A' },
      { url: 'http://localhost:5173/', title: 'Dev' },
    ],
    active: 1,
  })
  // The shown tab not kept (private): the first is the one that shows.
  assert.equal(sessionOf(tabs, 'p').active, 0)
})

test('a saved session comes back as waiting entries, malformed rows dropped', () => {
  const { tabs, active } = restoredTabs({
    tabs: [{ url: 'https://a.test/', title: 'A' }, null, { url: 'javascript:x' }, { url: 'https://b.test/' }],
    active: 5,
  })
  assert.deepEqual(tabs.map((t) => [t.url, t.title, t.native]), [
    ['https://a.test/', 'A', null],
    ['https://b.test/', '', null],
  ])
  assert.equal(active, 1, 'clamped')
  assert.ok(tabs.every(isDormant))
  assert.deepEqual(restoredTabs(null), { tabs: [], active: 0 })
})

test('a waiting tab is one with an address and no page, not opening or refused', () => {
  assert.equal(isDormant({ url: 'https://a.test/', native: null }), true)
  assert.equal(isDormant({ url: 'https://a.test/', native: 'tab-1' }), false)
  assert.equal(isDormant({ url: 'https://a.test/', loading: true }), false)
  assert.equal(isDormant({ url: 'https://a.test/', failed: true }), false)
  assert.equal(isDormant({ url: '' }), false)
  assert.equal(isDormant(null), false)
})

test("restored tabs go first; agents' tabs stay after them, and a tab the user is in stays shown (01aee56)", () => {
  const restored = restoredTabs({ tabs: [{ url: 'https://a.test/' }, { url: 'https://b.test/' }], active: 1 })
  const ntp = { key: 'ntp', url: '', native: null }
  const agent = { key: 'ag', url: 'https://agent.test/', native: 'tab-9', controller: { kind: 'agent' } }
  const after = withRestored([ntp, agent], 'ntp', restored)
  assert.deepEqual(after.tabs.map((t) => t.url), ['https://a.test/', 'https://b.test/', 'https://agent.test/'])
  assert.equal(after.active, restored.tabs[1].key, 'the session’s shown tab')
  const busy = withRestored([ntp, agent], 'ag', restored)
  assert.equal(busy.active, 'ag', 'the user is in the agent’s tab')
  // Nothing saved: nothing changes.
  const none = withRestored([ntp], 'ntp', { tabs: [], active: 0 })
  assert.deepEqual(none, { tabs: [ntp], active: 'ntp' })
})

test('On startup reads continue unless set to new_tab', () => {
  assert.equal(BROWSER_DEFAULTS.startup, 'continue')
  assert.deepEqual(STARTUP_CHOICES.map((c) => c.id), ['continue', 'new_tab'])
  assert.equal(browserSetting({}, 'startup'), 'continue')
  assert.equal(browserSetting({ startup: 'new_tab' }, 'startup'), 'new_tab')
  assert.equal(browserSetting({ startup: 'whatever' }, 'startup'), 'continue')
})

test('the store brings the session back once per run, then saves changes (never before)', async () => {
  const storage = new Map()
  const previous = globalThis.sessionStorage
  globalThis.sessionStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, String(v)) }
  try {
    const ntp = { key: 'ntp0', url: '', native: null }
    const agent = { key: 'ag0', url: 'https://agent.test/', native: 'tab-9' }
    setState(() => ({ tabs: [ntp, agent], active: 'ntp0' }))
    const sent = []
    const daemon = async (method, path, body) => {
      if (method === 'GET') {
        return {
          settings: {},
          permissions: {},
          session: { tabs: [{ url: 'https://a.test/', title: 'A' }, { url: 'https://b.test/', title: 'B' }], active: 0 },
        }
      }
      sent.push([method, path, body])
      return { ok: true }
    }
    await startSession(daemon)
    assert.equal(storage.get('openremote-browser-restored'), '1')
    assert.deepEqual(getState().tabs.map((t) => t.url), ['https://a.test/', 'https://b.test/', 'https://agent.test/'])
    assert.equal(getState().active, getState().tabs[0].key)
    // A second call in the run brings nothing back again.
    await startSession(daemon)
    assert.equal(getState().tabs.length, 3)
    // A change is saved after a moment; the agent's tab is in it, like any open tab.
    setState((s) => ({ active: s.tabs[1].key }))
    await new Promise((r) => setTimeout(r, 1200))
    assert.deepEqual(sent.at(-1), [
      'POST',
      '/browser/session',
      {
        tabs: [
          { url: 'https://a.test/', title: 'A' },
          { url: 'https://b.test/', title: 'B' },
          { url: 'https://agent.test/', title: '' },
        ],
        active: 1,
      },
    ])
  } finally {
    globalThis.sessionStorage = previous
  }
})

test('the view opens a waiting tab when shown; Settings offers On startup; a reload takes the pages back whole', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /if \(!hasTauri \|\| isMobile \|\| !isDormant\(current\)\) return\s+openInRef\.current\(current, current\.url\)/)
  assert.match(view, /if \(hasTauri && !isMobile\) startSession\(\)/)
  assert.match(view, /failed: !native/)
  const store = read('./browser-store.js')
  assert.match(store, /if \(restoredThisRun\(\)\) setState\(\(\) => \(\{ tabs, active: restoredActive\(tabs, wasActive\(\)\) \}\)\)\s+else setState\(\(s\) => \(\{ tabs: \[\.\.\.s\.tabs, \.\.\.tabs\] \}\)\)/)
  assert.match(store, /browserSetting\(prefs\.settings, 'startup'\) === 'continue'/)
  const settings = read('./SettingsView.jsx')
  assert.match(settings, /data-stg="browser-startup"/)
  assert.match(settings, /changeBrowserSetting\('startup', id\)/)
  assert.match(settings, /keys\.add\('browser-startup'\)/)
  for (const word of ['on startup', 'restore', 'session']) {
    assert.ok(searchSettings(word, SETTINGS_ENTRIES).some((e) => e.key === 'browser-startup'), word)
  }
})

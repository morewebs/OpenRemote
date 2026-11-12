// The browser's stored state has one piece that outlives a console reload:
// which page the user was on. The shell hands the pages back
// (`browser_tabs`) and the console used to land on the first one, so a
// development reload moved the user to a different tab and covered the
// console with it while it booted. These two cases pin the rule.

import assert from 'node:assert/strict'
import test from 'node:test'
import { getState, restoredActive, setState } from './browser-store.js'

const KEY = 'openremote-browser-active'

function fakeStorage() {
  const store = new Map()
  return {
    store,
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => store.set(key, value),
    removeItem: (key) => store.delete(key),
  }
}

test('switching tabs records the page, so a reload can come back to it', () => {
  const previous = globalThis.sessionStorage
  const storage = fakeStorage()
  globalThis.sessionStorage = storage
  try {
    setState(() => ({
      tabs: [
        { key: 'a', native: 'tab-1' },
        { key: 'b', native: 'tab-2' },
      ],
      active: 'b',
    }))
    assert.equal(storage.store.get(KEY), 'tab-2')
    // An event that does not change the active tab writes nothing new: a
    // page's title and loading state fire constantly during a navigation.
    setState((s) => ({ tabs: s.tabs.map((t) => ({ ...t, title: 'x' })) }))
    assert.equal(storage.store.get(KEY), 'tab-2')
    setState((s) => ({ active: 'a' }))
    assert.equal(storage.store.get(KEY), 'tab-1')
  } finally {
    globalThis.sessionStorage = previous
    // The store is a module singleton: leave it the way the next test in
    // this file expects to find it.
    setState((s) => ({ tabs: s.tabs, active: s.tabs[0]?.key }))
  }
  assert.ok(getState().tabs.length > 0)
})

test('a page that is gone falls back to the first tab', () => {
  const tabs = [
    { key: 'x', native: 'tab-7' },
    { key: 'y', native: 'tab-9' },
  ]
  assert.equal(restoredActive(tabs, 'tab-9'), 'y')
  // Closed in the shell while the console was loading: land somewhere real.
  assert.equal(restoredActive(tabs, 'tab-3'), 'x')
  assert.equal(restoredActive(tabs, null), 'x')
  assert.equal(restoredActive([], 'tab-1'), undefined)
})

// ---- bookmarks: the daemon keeps them, the cache only paints first ----

import { inFolder } from './browser-bookmarks.js'
import { refreshBookmarks, removeBookmark, saveBookmark } from './browser-store.js'

function daemonEnv(fetchImpl) {
  const storage = fakeStorage()
  storage.store.set('openremote-daemon-url', 'http://127.0.0.1:1234')
  storage.store.set('openremote-daemon-token', 'a-token')
  globalThis.localStorage = storage
  globalThis.fetch = fetchImpl
  setState(() => ({ bookmarks: [], bookmarksStale: false }))
  return storage
}

function ok(body) {
  return async () => ({ ok: true, status: 200, text: async () => JSON.stringify(body) })
}

function refused(error) {
  return async () => ({ ok: false, status: 409, text: async () => JSON.stringify({ error }) })
}

async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  await new Promise((resolve) => setTimeout(resolve, 0))
}

test('a bookmark write goes through the daemon route, and its answer wins', async () => {
  const calls = []
  const storage = daemonEnv(async (url, init) => {
    calls.push({ url, init })
    return ok({ bookmark: JSON.parse(init.body).id ? JSON.parse(init.body) : null })()
  })
  try {
    const saved = saveBookmark({ url: 'https://example.com/', title: 'Example' })
    assert.ok(saved && saved.url === 'https://example.com/')
    // The row is drawn at once and sent; the route is the browser's own.
    const sent = calls.find((c) => c.url.endsWith('/browser/bookmarks'))
    assert.ok(sent, 'the write reaches the daemon')
    assert.equal(sent.init.method, 'POST')
    assert.ok(JSON.parse(sent.init.body).request_id, 'a named request, so a retry is one change')
    await settle()
    assert.equal(getState().bookmarksStale, false, 'the daemon took it')
    // The cache holds the same list for the next first paint.
    assert.ok(storage.store.get('openremote-browser-bookmarks').includes('example.com'))
  } finally {
    delete globalThis.localStorage
    delete globalThis.fetch
  }
})

test('a refused write puts the list back and says it is what it last saw', async () => {
  daemonEnv(ok({ bookmark: null }))
  globalThis.fetch = refused('the daemon said no')
  try {
    saveBookmark({ url: 'https://kept.test/', title: 'Kept' })
    await settle()
    const before = getState().bookmarks.length
    saveBookmark({ url: 'https://lost.test/', title: 'Lost' })
    await settle()
    assert.ok(!getState().bookmarks.some((m) => m.url === 'https://lost.test/'), 'the unsaved row is not drawn as saved')
    assert.equal(getState().bookmarks.length, before)
    assert.equal(getState().bookmarksStale, true, 'the bar can say the list is the cache')
  } finally {
    delete globalThis.localStorage
    delete globalThis.fetch
  }
})

test('the daemon rows win over the cache on the first read', async () => {
  daemonEnv(ok({ bookmarks: [{ id: 'daemon-1', url: 'https://from-daemon.test/', title: 'From A', folder: 'bar', position: 1 }] }))
  try {
    setState(() => ({ bookmarks: [{ id: 'cache-1', url: 'https://from-cache.test/', title: 'Cache', folder: 'bar', position: 1 }] }))
    assert.equal(await refreshBookmarks(), true)
    assert.deepEqual(getState().bookmarks.map((m) => m.url), ['https://from-daemon.test/'])
    assert.equal(getState().bookmarksStale, false)
  } finally {
    delete globalThis.localStorage
    delete globalThis.fetch
  }
})

test('a new bookmark joins its folder, and a delete removes exactly that row', async () => {
  const storage = daemonEnv(ok({ bookmark: null }))
  try {
    const one = saveBookmark({ url: 'https://one.test/', title: 'One' })
    const two = saveBookmark({ url: 'https://two.test/', title: 'Two' })
    const bar = inFolder(getState().bookmarks, 'bar')
    assert.ok(bar.some((m) => m.id === one.id) && bar.some((m) => m.id === two.id), 'both are in the bar')
    // The order the bar draws is the order the store rule gives the same list.
    assert.deepEqual(
      bar.map((m) => m.id),
      inFolder(bar, 'bar').map((m) => m.id),
      'the store rule is the one the bar uses',
    )
    removeBookmark(one.id)
    assert.ok(!getState().bookmarks.some((m) => m.id === one.id), 'the deleted row is gone')
    assert.ok(getState().bookmarks.some((m) => m.id === two.id), 'the other row stays')
    assert.ok(storage.store.get('openremote-browser-bookmarks').includes('two.test'))
  } finally {
    delete globalThis.localStorage
    delete globalThis.fetch
  }
})

test('a daemon with no bookmarks route says so, and one that is silent says that', async () => {
  // An older daemon, which is what a console that updated before its sidecar
  // sees: the route is not there, and "the daemon did not answer" would be a
  // different (and untrue) fact.
  daemonEnv(async () => ({ ok: false, status: 404, text: async () => '' }))
  try {
    assert.equal(await refreshBookmarks(), false)
    assert.equal(getState().bookmarksStale, true)
    assert.match(getState().bookmarksNote, /until OpenRemote is updated/)
  } finally {
    delete globalThis.localStorage
    delete globalThis.fetch
  }
  // No answer at all: the network never reached it.
  daemonEnv(async () => {
    throw new TypeError('fetch failed')
  })
  try {
    assert.equal(await refreshBookmarks(), false)
    assert.match(getState().bookmarksNote, /isn't responding/)
  } finally {
    delete globalThis.localStorage
    delete globalThis.fetch
  }
})

test('the bookmarks this window already had move into the daemon that holds none', async () => {
  // A window that kept its bookmarks before the daemon did: the rows it can
  // already see are pushed once, so an update never empties a full bar.
  const kept = [{ id: 'bm-kept', url: 'https://kept.test/', title: 'Kept', folder: 'bar', position: 1 }]
  let held = []
  const posted = []
  const storage = daemonEnv(async (url, init) => {
    if (init?.method === 'POST') {
      const mark = JSON.parse(init.body)
      posted.push(mark.url)
      held = [...held, { ...mark, position: 1 }]
      return ok({ bookmark: mark })()
    }
    return ok({ bookmarks: held })()
  })
  try {
    storage.store.set('openremote-browser-bookmarks', JSON.stringify({ v: 1, marks: kept }))
    setState(() => ({ bookmarks: kept }))
    assert.equal(await refreshBookmarks(), true)
    assert.deepEqual(posted, ['https://kept.test/'], 'the row was pushed once')
    assert.deepEqual(getState().bookmarks.map((m) => m.url), ['https://kept.test/'], 'and the bar still holds it')
    assert.equal(getState().bookmarksStale, false)
  } finally {
    delete globalThis.localStorage
    delete globalThis.fetch
  }
})
// Clearing history clears the daemon's reading record too (review fix,
// Task 5): the new tab page's most-visited shortcuts are ranked from the
// daemon's visits (GET /browser/tiles), so a cleared page must not stay a
// shortcut. Same range as this window's record, through the daemon's
// existing POST /browser/history/clear; this window's record goes first,
// whatever the daemon says, and a refusal is thrown for the caller to say.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

// The store reads localStorage at load; node has none.
globalThis.localStorage ??= {
  data: new Map(),
  getItem(k) {
    return this.data.has(k) ? this.data.get(k) : null
  },
  setItem(k, v) {
    this.data.set(k, String(v))
  },
  removeItem(k) {
    this.data.delete(k)
  },
}

const { clearHistory, clearHistorySince, getState, recordVisit } = await import('./browser-store.js')

function fakeDaemon({ refuse = null } = {}) {
  const calls = []
  const caller = async (method, path, body) => {
    calls.push([method, path, body])
    if (refuse) throw refuse
    return { ok: true }
  }
  return { calls, caller }
}

test('a ranged clear takes the same range off the daemon, and this window keeps what is older', async () => {
  await clearHistory(fakeDaemon().caller)
  recordVisit({ url: 'https://old.test/', title: 'Old', at: 1_000 })
  recordVisit({ url: 'https://new.test/', title: 'New', at: 5_000 })
  const daemon = fakeDaemon()
  await clearHistorySince(3_000, daemon.caller)
  assert.deepEqual(daemon.calls, [['POST', '/browser/history/clear', { since: 3_000 }]])
  assert.deepEqual(getState().history.map((v) => v.url), ['https://old.test/'])
})

test('clearing it all clears it all on the daemon', async () => {
  recordVisit({ url: 'https://a.test/', title: 'A', at: 1 })
  const daemon = fakeDaemon()
  await clearHistory(daemon.caller)
  assert.deepEqual(daemon.calls, [['POST', '/browser/history/clear', {}]])
  assert.deepEqual(getState().history, [])
  const ranged = fakeDaemon()
  await clearHistorySince(null, ranged.caller)
  assert.deepEqual(ranged.calls, [['POST', '/browser/history/clear', {}]])
})

test("a daemon refusal still clears this window's record, and is thrown to be said", async () => {
  recordVisit({ url: 'https://b.test/', title: 'B', at: 1 })
  const refusal = Object.assign(new Error('no'), { status: 500 })
  await assert.rejects(clearHistory(fakeDaemon({ refuse: refusal }).caller), refusal)
  assert.deepEqual(getState().history, [])
})

test('every way to clear history says a daemon refusal through errorCopy', () => {
  const dialog = read('./BrowserClearData.jsx')
  assert.match(dialog, /if \(history\) await clearHistorySince\(since\)/)
  assert.match(dialog, /setError\(errorCopy\(err\)\)/)
  assert.match(read('./BrowserView.jsx'), /onClear=\{\(\) => clearHistory\(\)\.catch\(\(err\) => notify\(errorCopy\(err\)\)\)\}/)
  assert.match(read('./SettingsView.jsx'), /clearHistory\(\)\s*\n\s*\.then\(\(\) => setBrowserNote\('History cleared\.'\)\)\s*\n\s*\.catch\(\(err\) => setBrowserNote\(errorCopy\(err\)\)\)/)
  // The daemon's tiles are ranked from the visits this route clears.
  const store = read('../daemon/crates/openremote-daemon/src/browser/store.rs')
  assert.match(store, /pub fn clear_history\(&mut self, since: Option<i64>\)/)
  const tiles = store.slice(store.indexOf('pub fn tiles('), store.indexOf('pub fn tiles(') + 2500)
  assert.match(tiles, /self\.visits\(\)/)
  assert.match(read('../daemon/crates/openremote-daemon/src/browser/routes.rs'), /\.route\("\/browser\/history\/clear", post\(clear_history\)\)/)
})

// Settings > Browser's additions: the search engine and the bookmarks bar
// (kept by the daemon, shared with the open browser), and Clear browsing
// data (the reading record over a time range, the profile's cookies and
// cache whole). The rules are tested; the views are pinned as source.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import {
  BROWSER_DEFAULTS,
  browserSetting,
  getBrowserPrefs,
  loadBrowserPrefs,
  setBrowserPref,
  subscribeBrowserPrefs,
} from './browser-prefs.js'
import { CLEAR_RANGES, sinceFor, visitsBefore } from './browser-history.js'
import { clearKinds } from './browser-chrome.js'
import { SETTINGS_ENTRIES, searchSettings } from './settings-search.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('a setting reads as saved, else its default; the bar is on unless turned off', () => {
  assert.deepEqual(BROWSER_DEFAULTS, { search: 'google', bookmarks_bar: true, startup: 'continue', agent_tabs: false })
  assert.equal(browserSetting({}, 'search'), 'google')
  assert.equal(browserSetting({ search: 'duckduckgo' }, 'search'), 'duckduckgo')
  // An engine this build does not list reads as the default, never undefined.
  assert.equal(browserSetting({ search: 'altavista' }, 'search'), 'google')
  assert.equal(browserSetting(null, 'bookmarks_bar'), true)
  assert.equal(browserSetting({ bookmarks_bar: false }, 'bookmarks_bar'), false)
  assert.equal(browserSetting({ bookmarks_bar: 'yes' }, 'bookmarks_bar'), false)
  // Telling every chat about the tabs is off unless turned on.
  assert.equal(browserSetting({}, 'agent_tabs'), false)
  assert.equal(browserSetting({ agent_tabs: true }, 'agent_tabs'), true)
  assert.equal(browserSetting({ agent_tabs: 'yes' }, 'agent_tabs'), false)
})

test('the cache loads the daemon\'s settings and tells its subscribers', async () => {
  let told = 0
  const stop = subscribeBrowserPrefs(() => told++)
  const asked = []
  await loadBrowserPrefs(async (method, path) => {
    asked.push(`${method} ${path}`)
    return { settings: { search: 'bing' }, permissions: { 'https://a.test|camera': 'allow' }, tabs: [] }
  })
  assert.deepEqual(asked, ['GET /browser'])
  assert.deepEqual(getBrowserPrefs().permissions, { 'https://a.test|camera': 'allow' })
  assert.equal(getBrowserPrefs().loaded, true)
  assert.equal(browserSetting(getBrowserPrefs().settings, 'search'), 'bing')
  // A failed read keeps what was there.
  await loadBrowserPrefs(async () => {
    throw new Error('no daemon')
  })
  assert.equal(browserSetting(getBrowserPrefs().settings, 'search'), 'bing')
  stop()
  assert.ok(told >= 1)
})

test('a change shows at once, is written, and goes back when the daemon refuses', async () => {
  const sent = []
  await setBrowserPref(async (method, path, body) => {
    sent.push([method, path, body])
    return { search: 'bing', bookmarks_bar: false }
  }, 'bookmarks_bar', false)
  assert.deepEqual(sent, [['POST', '/browser/settings', { bookmarks_bar: false }]])
  assert.equal(browserSetting(getBrowserPrefs().settings, 'bookmarks_bar'), false)

  let during = null
  const refusal = Object.assign(new Error('nope'), { status: 500 })
  await assert.rejects(
    setBrowserPref(async () => {
      during = browserSetting(getBrowserPrefs().settings, 'search')
      throw refusal
    }, 'search', 'brave'),
    refusal,
  )
  assert.equal(during, 'brave', 'shown while the write is out')
  assert.equal(browserSetting(getBrowserPrefs().settings, 'search'), 'bing', 'put back after the refusal')
})

test('Chrome\'s time ranges, and clearing keeps only what is older', () => {
  assert.deepEqual(CLEAR_RANGES.map((r) => r.id), ['hour', 'day', 'week', 'month', 'all'])
  const now = 10 * 24 * 60 * 60 * 1000
  assert.equal(sinceFor('hour', now), now - 60 * 60 * 1000)
  assert.equal(sinceFor('month', now), now - 28 * 24 * 60 * 60 * 1000)
  assert.equal(sinceFor('all', now), null)
  // A range this build does not know is refused, never read as All time.
  assert.throws(() => sinceFor('nonsense', now), RangeError)
  assert.throws(() => sinceFor(undefined, now), RangeError)
  const visits = [
    { url: 'https://c.test/', at: now - 1000 },
    { url: 'https://b.test/', at: now - 2 * 60 * 60 * 1000 },
    { url: 'https://a.test/', at: now - 3 * 24 * 60 * 60 * 1000 },
  ]
  assert.deepEqual(visitsBefore(visits, sinceFor('hour', now)).map((v) => v.url), ['https://b.test/', 'https://a.test/'])
  assert.deepEqual(visitsBefore(visits, sinceFor('day', now)).map((v) => v.url), ['https://a.test/'])
  assert.deepEqual(visitsBefore(visits, null), [])
  // Only null empties the record: anything that is not a time clears nothing.
  assert.equal(visitsBefore(visits, undefined).length, 3)
  assert.equal(visitsBefore(visits, NaN).length, 3)
})

test('the shell is asked for what was ticked: cookies with the sites\' storage', () => {
  assert.deepEqual(clearKinds({ cookies: true, cache: true }), ['cookies', 'storage', 'cache'])
  assert.deepEqual(clearKinds({ cache: true }), ['cache'])
  assert.deepEqual(clearKinds({}), [])
  // Every kind is one the shell's clear_data names.
  const win = read('../src-tauri/src/browser/win.rs')
  for (const kind of clearKinds({ cookies: true, cache: true })) assert.match(win, new RegExp(`"${kind}" =>`))
})

test('the new rows are registered and found by the words a person types', () => {
  for (const [word, key] of [
    ['search engine', 'browser-search'],
    ['duckduckgo', 'browser-search'],
    ['bookmarks bar', 'browser-bookmarks-bar'],
    ['tabs agents', 'browser-agent-tabs'],
    ['open tabs', 'browser-agent-tabs'],
    ['browsing data', 'browser-clear-data'],
  ]) {
    const rows = searchSettings(word, SETTINGS_ENTRIES)
    assert.ok(rows.some((e) => e.key === key), `"${word}" reaches ${key}`)
  }
})

test('Settings draws the rows, writes through the shared cache, and hides what cannot act', () => {
  const view = read('./SettingsView.jsx')
  for (const key of ['browser-search', 'browser-bookmarks-bar', 'browser-clear-data']) {
    assert.match(view, new RegExp(`data-stg="${key}"`), `${key} is drawn`)
  }
  assert.match(view, /setBrowserPref\(browserCall, key, value\)\.catch\(\(err\) => setBrowserNote\(errorCopy\(err\)\)\)/)
  assert.match(view, /changeBrowserSetting\('search', id\)/)
  assert.match(view, /changeBrowserSetting\('bookmarks_bar', !barEverywhere\)/)
  // The daemon reads this key (browser/mod.rs AGENT_TABS) to tell every chat.
  assert.match(view, /data-stg="browser-agent-tabs"/)
  assert.match(view, /changeBrowserSetting\('agent_tabs', !agentTabs\)/)
  assert.match(view, /keys\.add\('browser-agent-tabs'\)/)
  assert.match(read('../daemon/crates/openremote-daemon/src/browser/mod.rs'), /pub const AGENT_TABS: &str = "agent_tabs";/)
  // The settings need the desktop app's browser; clearing needs a shell that can.
  assert.match(view, /if \(!hasTauri \|\| isMobile\) \{\s*keys\.add\('browser-search'\)\s*keys\.add\('browser-bookmarks-bar'\)/)
  assert.match(view, /if \(!profileClearable\) keys\.add\('browser-clear-data'\)/)
  assert.match(view, /setProfileClearable\(Boolean\(features\?\.clear_site\)\)/)
  assert.match(view, /\{profileClearable && \(\s*<div className="pn-row" data-stg="browser-clear-data">/)
})

test('the dialog is the dialog family, clears through the shell, and says refusals through errorCopy', () => {
  const dialog = read('./BrowserClearData.jsx')
  assert.match(dialog, /useDialog\(panelRef, onClose\)/)
  assert.match(dialog, /invoke\('browser_clear_data', \{ kinds \}\)/)
  // The range is read before anything is cleared, so a bad one stops the lot.
  assert.match(dialog, /const since = history \? sinceFor\(range\) : null\s*const kinds = clearKinds/)
  assert.match(dialog, /if \(history\) await clearHistorySince\(since\)/)
  assert.match(dialog, /setError\(errorCopy\(err\)\)/)
  assert.doesNotMatch(dialog, /addEventListener\('keydown'/)
})

test('the browser searches with the chosen engine and shows the bar as chosen', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /const url = omnibox\(line, engine\)/)
  assert.match(view, /openUrl\(omnibox\(row\.url, engine\)/)
  assert.match(view, /\{bookmarks\.length > 0 && \(barEverywhere \|\| !live\) && \(/)
  assert.match(view, /placeholder=\{`Search \$\{SEARCH_ENGINES\[engine\]\.name\} or type a URL`\}/)
})

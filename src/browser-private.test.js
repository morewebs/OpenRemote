// Private tabs (ported from the browser branch's 0d3211a): Ctrl+Shift+N
// opens one, its page is built in the shell's throwaway profile, it wears
// the Detective mark, leaves no trace in the closed list or history, and
// agents see it only when the user hands it over from the site menu.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { closedEntry, rememberClosed } from './browser-tabs.js'
import { BROWSER_ACTIONS, browserBindings, actionForCombo } from './shortcuts.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('a private tab leaves nothing for the closed list', () => {
  assert.equal(closedEntry({ url: 'https://example.com/', title: 'Ex', private: true }), null)
  const before = [{ url: 'https://a.test/', title: 'A' }]
  assert.equal(rememberClosed(before, { url: 'https://b.test/', private: true }), before)
  // A normal tab still does.
  assert.equal(rememberClosed([], { url: 'https://b.test/', title: 'B' })[0].url, 'https://b.test/')
})

test('Ctrl+Shift+N opens a private tab in the browser, and the shell hands it over from a page', () => {
  const action = BROWSER_ACTIONS.find((a) => a.id === 'browser-private-tab')
  assert.equal(action?.keys, 'Mod+Shift+N')
  assert.equal(actionForCombo(browserBindings({}), 'Mod+Shift+N'), 'browser-private-tab')
  const win = read('../src-tauri/src/browser/win.rs')
  assert.match(win, /"Mod\+Shift\+N"/)
})

test('the view opens private pages privately and lets the user share one with agents', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /call\('browser_open', \{ url, bounds, show: false, private: Boolean\(tab\.private\) \}\)/)
  assert.match(view, /'browser-private-tab': openPrivate/)
  assert.match(view, /label: 'New private tab'/)
  assert.match(view, /call\('browser_share', \{ tab: tab\.native, on \}\)/)
  assert.match(view, /'Let agents use this tab'/)
  // The mark in the strip, the address bar and the new tab page.
  assert.match(view, /t\.private \? ' private' : ''/)
  assert.match(view, /<Detective size=\{14\} className="bw-tab-icon" aria-label="Private" \/>/)
  assert.match(view, /className="bw-private"/)
  assert.match(view, /className="bw-ntp-private"/)
  // History stays closed to it (the visit effect returns first).
  assert.match(view, /if \(current\?\.private\) return\s+const url = current\?\.url/)
  // The new tab button hands openTab nothing (08d720e): a click event is not a tab's fields.
  assert.match(view, /className="bw-newtab" title="New tab \(Ctrl\+T\)" onClick=\{\(\) => openTab\(\)\}/)
})

test('the store keeps what the shell says about a private tab and its sharing', () => {
  const store = read('./browser-store.js')
  assert.match(store, /private: p\.private === true/)
  assert.match(store, /shared: p\.shared === true/)
  const css = read('./browser.css')
  for (const rule of ['.bw-tab.private .bw-tab-icon', '.bw-private {', '.bw-ntp-private {']) {
    assert.ok(css.includes(rule), rule)
  }
})

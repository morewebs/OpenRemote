import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { omnibox, security } from './browser-omnibox.js'
import { closeTab, canUse, cycle, extrasMissing, insertAfter, newTab, openBelongs, popupBelongs, tabTitle } from './browser-tabs.js'
import { covered } from './browser-occlusion.js'
import { DEVICES, fileName, fitDevice, nextZoom, zoomLabel } from './browser-chrome.js'

test('a device size fits the page area without changing shape', () => {
  const phone = DEVICES[0]
  assert.deepEqual(fitDevice(phone, { width: 1000, height: 1000 }), { width: 393, height: 852, scale: 1 })
  const fit = fitDevice(phone, { width: 1000, height: 426 })
  assert.equal(fit.height, 426)
  assert.equal(fit.width, 197)
  assert.equal(fit.scale, 0.5)
})

test("zoom steps follow Chrome's, and reset to 100%", () => {
  assert.equal(nextZoom(1, 1), 1.1)
  assert.equal(nextZoom(1, -1), 0.9)
  assert.equal(nextZoom(5, 1), 5)
  assert.equal(nextZoom(0.25, -1), 0.25)
  assert.equal(nextZoom(1.5, 0), 1)
  assert.equal(zoomLabel(1.25), '125%')
})

test('a download is named after its file', () => {
  assert.equal(fileName('C:\\Users\\me\\Downloads\\report (1).pdf'), 'report (1).pdf')
  assert.equal(fileName('https://example.com/files/My%20Doc.zip?x=1'), 'My Doc.zip')
  assert.equal(fileName(''), 'Download')
})

test('the address bar opens addresses and searches the rest', () => {
  assert.equal(omnibox('example.com'), 'https://example.com')
  assert.equal(omnibox('github.com/morewebs/openremote'), 'https://github.com/morewebs/openremote')
  assert.equal(omnibox('https://a.dev/x?y=1'), 'https://a.dev/x?y=1')
  assert.equal(omnibox('http://example.com'), 'http://example.com')
  // the user's own dev servers: plain http
  assert.equal(omnibox('localhost:5173'), 'http://localhost:5173')
  assert.equal(omnibox('127.0.0.1:3000/api'), 'http://127.0.0.1:3000/api')
  assert.equal(omnibox('localhost'), 'http://localhost')
  // words are a search, Google by default
  assert.equal(omnibox('how to center a div'), 'https://www.google.com/search?q=how%20to%20center%20a%20div')
  assert.equal(omnibox('rust'), 'https://www.google.com/search?q=rust')
  assert.equal(omnibox('node.js streams', 'duckduckgo'), 'https://duckduckgo.com/?q=node.js%20streams')
  // nothing, or a script, opens nothing
  assert.equal(omnibox('   '), null)
  assert.equal(omnibox('javascript:alert(1)'), null)
  assert.equal(omnibox('about:blank'), 'about:blank')
})

test('the lock tells secure, local and insecure pages apart', () => {
  assert.equal(security('https://example.com'), 'secure')
  assert.equal(security('http://localhost:5173/'), 'local')
  assert.equal(security('http://example.com'), 'insecure')
  assert.equal(security(''), 'none')
})

test('closing a tab activates its right neighbour, and the last leaves a new tab', () => {
  const [a, b, c] = [newTab({ url: 'a' }), newTab({ url: 'b' }), newTab({ url: 'c' })]
  let next = closeTab([a, b, c], b.key, b.key)
  assert.deepEqual(next.tabs.map((t) => t.url), ['a', 'c'])
  assert.equal(next.active, c.key)
  assert.equal(next.closed.url, 'b')
  next = closeTab([a, b, c], c.key, c.key)
  assert.equal(next.active, b.key)
  // closing an inactive tab keeps the active one
  next = closeTab([a, b, c], a.key, c.key)
  assert.equal(next.active, c.key)
  next = closeTab([a], a.key, a.key)
  assert.equal(next.tabs.length, 1)
  assert.notEqual(next.tabs[0].key, a.key)
  assert.equal(next.active, next.tabs[0].key)
})

test('tabs cycle with wrap-around and popups land beside their opener', () => {
  const [a, b, c] = [newTab(), newTab(), newTab()]
  assert.equal(cycle([a, b, c], c.key, 1), a.key)
  assert.equal(cycle([a, b, c], a.key, -1), c.key)
  const p = newTab({ url: 'popup' })
  assert.deepEqual(insertAfter([a, b, c], p, a.key).map((t) => t.key), [a.key, p.key, b.key, c.key])
  assert.equal(tabTitle({ title: '', url: 'https://example.com/x' }), 'example.com/x')
  assert.equal(tabTitle({ title: '', url: '' }), 'New tab')
})

test('a menu overlapping the page swaps in its picture; one beside it does not', () => {
  const page = { left: 300, top: 100, right: 1200, bottom: 800, width: 900, height: 700 }
  assert.equal(covered(page, [{ left: 1100, top: 50, right: 1400, bottom: 300, width: 300, height: 250 }]), true)
  assert.equal(covered(page, [{ left: 0, top: 0, right: 280, bottom: 900, width: 280, height: 900 }]), false)
  assert.equal(covered(page, []), false)
})

test('a page that opens after its tab or its pane is gone is closed, not shown', () => {
  const [a, b] = [newTab(), newTab()]
  assert.equal(openBelongs([a, b], a.key, true), true)
  // The tab was closed while the shell was building the page.
  assert.equal(openBelongs([b], a.key, true), false)
  // The pane was closed while the shell was building the page.
  assert.equal(openBelongs([a, b], a.key, false), false)
})

test('the pane is what closes a page it cannot place, and drops the spinner with it', () => {
  // The rule above is only worth having where it is applied: a page that
  // lands after its tab or its pane is gone has to be closed by the pane's
  // own open path, and the tab that was waiting for it has to stop loading
  // instead of spinning for the session. Neither can be reached from here
  // (the pane needs a real window and the shell), so this reads the call
  // site: delete the check, or leave `loading` on, and this fails.
  const source = readFileSync(new URL('./BrowserView.jsx', import.meta.url), 'utf8')
  assert.match(
    source,
    /if \(!native \|\| !openBelongs\(getState\(\)\.tabs, tab\.key, mounted\.current\)\) \{/,
  )
  assert.match(source, /patchTab\(tab\.key, \{ loading: false, failed: !native \}\)/)
})

test('a popup lands beside a tab this console has, and nowhere otherwise', () => {
  const [a, b] = [newTab({ native: 'tab-1' }), newTab({ native: 'tab-2' })]
  assert.equal(popupBelongs([a, b], 'tab-1'), true)
  // The opener closed, or this console never had it: no tab will place the
  // page, so the console closes it rather than leaving it in the shell's
  // list to reappear as a stray tab on the next load.
  assert.equal(popupBelongs([a], 'tab-2'), false)
  assert.equal(popupBelongs([a, b], null), false)
})

test('a capability that was never answered reads as unavailable, not as available', () => {
  // The shell's answer or nothing: unknown must not be yes, or a control the
  // shell would refuse is drawn as if it worked and its click does nothing.
  assert.equal(canUse(null, 'find'), false)
  assert.equal(canUse(undefined, 'devtools'), false)
  assert.equal(canUse({}, 'viewport'), false, 'the store keeps a failed ask as an empty answer')
  assert.equal(canUse({ find: true }, 'find'), true)
  assert.equal(canUse({ find: false }, 'find'), false)
  assert.equal(canUse({ find: true }, 'devtools'), false, 'a key that is absent is not a yes')
})

test('the extras line is for an answer with no extras, not for a missing answer', () => {
  assert.equal(extrasMissing(null), false, 'nothing to say before the shell has answered')
  assert.equal(extrasMissing({}), true, 'a failed ask is exactly when the line is owed')
  assert.equal(extrasMissing({ find: false, devtools: false }), true)
  assert.equal(extrasMissing({ find: true, devtools: false }), false)
})

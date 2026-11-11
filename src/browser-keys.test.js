// The browser's keys in the shortcut catalog: a Browser group, its own
// scope (a key moves only between actions of one scope), every default one
// the shell hands over from a page (TAKEN in src-tauri/src/browser/win.rs),
// and listed only where there is a browser.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import {
  ACTIONS,
  BROWSER_ACTIONS,
  CATALOG,
  GROUPS,
  actionForCombo,
  browserBindings,
  defaultKey,
  effectiveBindings,
  rebind,
  resetOne,
  shownActions,
  staleOverrides,
} from './shortcuts.js'
import { SETTINGS_ENTRIES, searchSettings } from './settings-search.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('the browser keys are a Browser group of their own, after the app', () => {
  assert.equal(GROUPS.at(-1).id, 'browser')
  assert.ok(BROWSER_ACTIONS.length > 0)
  for (const a of BROWSER_ACTIONS) {
    assert.equal(a.group, 'browser')
    assert.match(a.id, /^browser-/)
  }
  assert.deepEqual(CATALOG, [...ACTIONS, ...BROWSER_ACTIONS])
  assert.ok(!ACTIONS.some((a) => a.group === 'browser'))
  assert.deepEqual(shownActions(false), ACTIONS)
  assert.deepEqual(shownActions(true), CATALOG)
})

test('the app and the browser each hold their own keys: Ctrl+W is both', () => {
  const app = effectiveBindings({})
  const browser = browserBindings({})
  assert.equal(app['close-pane'], 'Mod+W')
  assert.equal(browser['browser-close-tab'], 'Mod+W')
  assert.equal(app['focus-composer'], 'Mod+L')
  assert.equal(browser['browser-address'], 'Mod+L')
  assert.equal(actionForCombo(browser, 'Mod+T'), 'browser-new-tab')
  assert.equal(actionForCombo(app, 'Mod+T'), null)
  // The app's dispatch never sees a browser action.
  assert.ok(!Object.keys(app).some((id) => id.startsWith('browser-')))
})

test('show or hide the browser keeps Ctrl+Shift+B; no browser action takes it', () => {
  assert.equal(effectiveBindings({})['toggle-browser'], 'Mod+Shift+B')
  assert.equal(actionForCombo(browserBindings({}), 'Mod+Shift+B'), null)
})

test('taking a key moves it only inside its scope', () => {
  // A browser action taking Ctrl+T moves it off New tab...
  const { overrides, displaced } = rebind({}, 'browser-history', 'Mod+T')
  assert.deepEqual(displaced, ['browser-new-tab'])
  assert.equal(browserBindings(overrides)['browser-new-tab'], null)
  // ...and a browser key that is also the app's leaves the app's alone.
  const closing = rebind({}, 'browser-find', 'Mod+W')
  assert.deepEqual(closing.displaced, ['browser-close-tab'])
  assert.equal(effectiveBindings(closing.overrides)['close-pane'], 'Mod+W')
  // An app action taking Ctrl+T does not touch the browser's.
  const app = rebind({}, 'archive', 'Mod+T')
  assert.deepEqual(app.displaced, [])
  assert.equal(browserBindings(app.overrides)['browser-new-tab'], 'Mod+T')
  // Reset gives the key back within the scope.
  const back = resetOne(overrides, 'browser-new-tab')
  assert.equal(browserBindings(back)['browser-new-tab'], 'Mod+T')
  assert.equal(browserBindings(back)['browser-history'], null)
})

test('a saved browser key is a known action, not a stale one', () => {
  assert.deepEqual(staleOverrides({ 'browser-new-tab': 'Mod+Alt+T' }), [])
})

test('the Mac gets Chrome\'s Mac keys where Cmd+Tab or Option+arrows would clash', () => {
  assert.equal(defaultKey('browser-next-tab', true), 'Ctrl+Tab')
  assert.equal(defaultKey('browser-back', true), 'Mod+[')
  assert.equal(defaultKey('browser-close-tab', true), 'Mod+Alt+W')
  assert.equal(defaultKey('browser-next-tab', false), 'Mod+Tab')
})

test('every default browser key is one the shell takes from a page (TAKEN)', () => {
  const win = read('../src-tauri/src/browser/win.rs')
  const block = /const TAKEN: &\[&str\] = &\[([\s\S]*?)\];/.exec(win)
  assert.ok(block, 'win.rs declares TAKEN')
  const taken = new Set([...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]))
  for (const a of BROWSER_ACTIONS) {
    assert.ok(taken.has(a.keys), `${a.id} (${a.keys}) is in TAKEN`)
  }
  // The app keys a page hands back keep their meaning (replayed, not run as
  // browser actions): none is a browser default.
  const view = read('./BrowserView.jsx')
  const appKeys = /const APP_KEYS = \{([\s\S]*?)\n\}/.exec(view)[1]
  for (const [, chord] of appKeys.matchAll(/'([^']+)':/g)) {
    assert.equal(actionForCombo(browserBindings({}), chord), null, `${chord} is not a browser default`)
  }
})

test('Settings finds the browser keys by name', () => {
  const rows = searchSettings('reopen the closed tab', SETTINGS_ENTRIES)
  assert.ok(rows.some((e) => e.key === 'keyboard-browser-reopen-tab'))
})

test('the browser runs its keys from the catalog, and only while it has focus', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /bound\.current = kb\?\.browserBindings \?\? browserBindings\(\{\}, isMac\)/)
  assert.match(view, /keys\.current\[actionForCombo\(bound\.current, chord\)\]/)
  for (const a of BROWSER_ACTIONS) assert.ok(view.includes(`'${a.id}':`), `the view handles ${a.id}`)
  assert.match(view, /if \(!rootRef\.current\?\.contains\(e\.target\)\) return/)
  assert.match(view, /<div className="bw" ref=\{rootRef\}>/)
  assert.doesNotMatch(view, /function chordOf/)
})

test('the keyboard layer dispatches the app\'s keys and lists both', () => {
  const kb = read('./keyboard.jsx')
  assert.match(kb, /const bindingsRef = useRef\(appBindings\)/)
  assert.match(kb, /browserBindings: browserKeys,/)
})

test('Settings and the cheat sheet list the Browser group only in the desktop app', () => {
  const settings = read('./SettingsView.jsx')
  const sheet = read('./ShortcutSheet.jsx')
  for (const src of [settings, sheet]) {
    assert.match(src, /const BROWSER_KEYS = hasTauri && !isMobile/)
    assert.match(src, /shownActions\(BROWSER_KEYS\)/)
  }
  assert.match(settings, /if \(!BROWSER_KEYS\) for \(const a of BROWSER_ACTIONS\) keys\.add\(`keyboard-\$\{a\.id\}`\)/)
  assert.match(settings, /These act while the browser has focus\./)
})

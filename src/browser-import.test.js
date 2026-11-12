// Importing from another browser in Settings > Browser: the daemon's list
// read defensively, the result said plainly (the cap and the skipped entries
// included), the calls going to the daemon's own route, and the row and its
// dialog drawn by the console's rules.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { importLine, loadSources, readSources, runImport, sourceLabel } from './browser-import.js'
import { SETTINGS_ENTRIES } from './settings-search.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('the profiles list keeps only what the daemon said, and what can be imported', () => {
  for (const raw of [null, undefined, 'x', [], {}, { sources: 'no' }]) assert.deepEqual(readSources(raw), [])
  const listed = readSources({
    sources: [
      { id: 'chrome-default', browser: 'Chrome', profile: 'Default', bookmarks: true, history: true },
      { id: 'edge-default', browser: 'Edge', profile: 'Default', bookmarks: false, history: false },
      { id: 'safari', browser: 'Safari', profile: 'Safari', bookmarks: 'yes', history: true },
      { browser: 'No id' },
      null,
    ],
  })
  assert.deepEqual(
    listed.map((s) => [s.id, s.bookmarks, s.history]),
    [
      ['chrome-default', true, true],
      ['safari', false, true],
    ],
  )
  assert.equal(sourceLabel(listed[0]), 'Chrome (Default)')
  assert.equal(sourceLabel(listed[1]), 'Safari')
})

test('the line says what came in, and what was left out and why', () => {
  assert.equal(
    importLine({ browser: 'Chrome', bookmarks: 4, visits: 144, already: 1, refused: 3 }),
    'Imported 4 bookmarks (in Other bookmarks) and 144 pages of history from Chrome. 1 bookmark was already here. 3 entries were skipped for not being a web page (scripts, local files, inline data).',
  )
  assert.equal(importLine({ browser: 'Firefox', bookmarks: 0, visits: 0, already: 5 }), 'Nothing new to import from Firefox. 5 bookmarks were already here.')
  assert.match(importLine({ bookmarks: 2000, over_cap: 500 }), /500 bookmarks were left out: one import adds at most 2,000/)
  assert.equal(importLine(null), 'Nothing new to import.')
  assert.doesNotMatch(importLine({ bookmarks: 1, visits: 1 }), /\u2014/)
})

test('reads and runs go to the daemon route, and a refusal is thrown', async () => {
  const calls = []
  const call = async (method, path, body) => {
    calls.push([method, path, body ?? null])
    return { sources: [] }
  }
  await loadSources(call)
  await runImport(call, { source: 'chrome-default' })
  await runImport(call, { path: 'C:\\Users\\me\\Profile', history: false })
  assert.deepEqual(calls, [
    ['GET', '/browser/import', null],
    ['POST', '/browser/import', { bookmarks: true, history: true, source: 'chrome-default' }],
    ['POST', '/browser/import', { bookmarks: true, history: false, path: 'C:\\Users\\me\\Profile' }],
  ])
  const refused = Object.assign(new Error('An import is already running.'), { status: 409 })
  await assert.rejects(runImport(async () => { throw refused }, { source: 'x' }), refused)
})

test('the row is in Settings > Browser, registered for search, and leaves search where it is not drawn', () => {
  const entry = SETTINGS_ENTRIES.find((e) => e.key === 'browser-import')
  assert.ok(entry)
  assert.equal(entry.section, 'browser')
  for (const word of ['import', 'chrome', 'firefox', 'safari']) assert.ok(entry.terms.includes(word), word)
  const view = read('./SettingsView.jsx')
  assert.ok(view.includes('data-stg="browser-import"'))
  assert.ok(view.includes("keys.add('browser-import')"))
  assert.match(view, /<BrowserImport onClose=/)
})

test('the import dialog follows the dialog rules and says what is never read', () => {
  const src = read('./BrowserImport.jsx')
  // Esc, Back, the X and the backdrop all go through one close that does
  // nothing while an import runs: the dialog stays to say how it went.
  assert.match(src, /useDialog\(panelRef, close\)/)
  assert.match(src, /useBack\(true, close\)/)
  assert.match(src, /const close = \(\) => \{\s*if \(busyRef\.current\) return/)
  assert.match(src, /aria-label="Close" onClick=\{close\} disabled=\{busy\}/)
  assert.match(src, /if \(e\.target === e\.currentTarget\) close\(\)/)
  assert.doesNotMatch(src, /onClick=\{onClose\}/)
  assert.match(src, /role="dialog"/)
  assert.match(src, /setError\(errorCopy\(err\)\)/)
  assert.doesNotMatch(src, /addEventListener\('keydown'/)
  assert.match(src, /never sign-ins, passwords or site data/)
  // A folder is only ever one the user picked in the system dialog.
  assert.match(src, /pickFolder\(/)
  assert.doesNotMatch(src, /<input[^>]*type="text"/)
  // What came in reaches the bar at once.
  assert.match(src, /refreshBookmarks\(\)/)
})

test('bookmarks are not said to stay on this computer: they follow the account', () => {
  const row = SETTINGS_ENTRIES.find((e) => e.key === 'browser-bookmarks')
  assert.doesNotMatch(row.detail, /Nothing is sent anywhere/)
  assert.match(row.detail, /account/)
  const view = read('./SettingsView.jsx')
  assert.doesNotMatch(view, /sent anywhere/)
  assert.match(view, /follow your account/)
})

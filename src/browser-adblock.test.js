// Ad and tracker blocking in Settings > Browser: off with no list picked,
// a licence beside every list on offer, the daemon's words quoted, and the
// rows registered for search and drawn through the dialog rules.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import {
  adblockLine,
  listLabel,
  loadAdblock,
  pickedIds,
  readAdblock,
  setAdblock,
  setAdblockSite,
  siteOf,
} from './browser-adblock.js'
import { SETTINGS_ENTRIES } from './settings-search.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

const CATALOG = [
  { id: 'easylist', title: 'EasyList', about: 'Adverts', licence: 'GPL-3.0-or-later or CC BY-SA 3.0+', picked: false },
  { id: 'easyprivacy', title: 'EasyPrivacy', about: 'Trackers', licence: 'GPL-3.0-or-later or CC BY-SA 3.0+', picked: false },
]

test('anything the daemon did not say reads as off, nothing picked', () => {
  for (const raw of [null, undefined, 'x', [], {}]) {
    const s = readAdblock(raw)
    assert.equal(s.on, false)
    assert.equal(s.supported, false)
    assert.equal(s.state, 'off')
    assert.deepEqual(s.lists, [])
    assert.deepEqual(s.sites, [])
  }
  const s = readAdblock({ lists: [{ id: 'x' }, null, { id: 'y', title: 'Y', picked: 'yes' }] })
  assert.deepEqual(s.lists.map((l) => [l.id, l.picked]), [['y', false]])
})

test('every list is named with its licence beside it', () => {
  const s = readAdblock({ lists: CATALOG })
  assert.deepEqual(
    s.lists.map(listLabel),
    ['EasyList (GPL-3.0-or-later or CC BY-SA 3.0+)', 'EasyPrivacy (GPL-3.0-or-later or CC BY-SA 3.0+)'],
  )
  assert.deepEqual(pickedIds(s), [])
})

test('the line says what blocking is doing, and quotes the daemon on a failure', () => {
  const off = readAdblock({ state: 'off', lists: CATALOG })
  assert.match(adblockLine(off), /Nothing is downloaded or blocked until you turn it on and pick/)
  const picked = readAdblock({ state: 'off', lists: [{ ...CATALOG[0], picked: true }] })
  assert.match(adblockLine(picked), /EasyList is picked; nothing is downloaded/)
  assert.match(adblockLine(readAdblock({ on: true, state: 'none', lists: CATALOG })), /no filter list is picked/)
  const lists = [{ ...CATALOG[0], picked: true }, { ...CATALOG[1], picked: true }]
  assert.match(adblockLine(readAdblock({ on: true, state: 'updating', lists })), /Downloading EasyList and EasyPrivacy/)
  assert.match(adblockLine(readAdblock({ on: true, state: 'on', lists })), /Blocking with EasyList and EasyPrivacy/)
  const failed = adblockLine(
    readAdblock({ on: true, state: 'error', error: 'easylist.to answered 503.', lists }),
  )
  assert.match(failed, /failed: “easylist\.to answered 503”\./)
})

test('a site is a host name, whatever was pasted', () => {
  assert.equal(siteOf('Example.com'), 'example.com')
  assert.equal(siteOf('https://News.Example.com/path?q=1'), 'news.example.com')
  assert.equal(siteOf('example.com/path'), 'example.com')
  assert.equal(siteOf('example.com:8080'), 'example.com')
  assert.equal(siteOf('example.com.'), 'example.com')
  assert.equal(siteOf(''), null)
  assert.equal(siteOf('exa mple.com'), null)
  assert.equal(siteOf('a..b'), null)
  assert.equal(siteOf('javascript:alert(1)'), null)
})

test('reads and writes go to the daemon routes, and a refusal is thrown', async () => {
  const calls = []
  const call = async (method, path, body) => {
    calls.push([method, path, body ?? null])
    return { on: true, state: 'none', lists: CATALOG }
  }
  await loadAdblock(call)
  await setAdblock(call, { on: true })
  await setAdblock(call, { lists: ['easylist'] })
  await setAdblockSite(call, 'example.com', true)
  assert.deepEqual(calls, [
    ['GET', '/browser/adblock', null],
    ['POST', '/browser/adblock', { on: true }],
    ['POST', '/browser/adblock', { lists: ['easylist'] }],
    ['POST', '/browser/adblock/site', { site: 'example.com', allow: true }],
  ])
  const refused = Object.assign(new Error('nope is not a filter list this app offers'), { status: 400 })
  await assert.rejects(setAdblock(async () => { throw refused }, { lists: ['nope'] }), refused)
})

test('the rows are in Settings > Browser and registered for search', () => {
  const keys = SETTINGS_ENTRIES.filter((e) => e.section === 'browser').map((e) => e.key)
  for (const key of ['browser-adblock', 'browser-adblock-lists', 'browser-adblock-sites']) {
    assert.ok(keys.includes(key), key)
  }
  const view = read('./SettingsView.jsx')
  for (const key of ['browser-adblock', 'browser-adblock-lists', 'browser-adblock-sites']) {
    assert.ok(view.includes(`data-stg="${key}"`), `${key} is drawn`)
    assert.ok(view.includes(`keys.add('${key}')`), `${key} leaves search where it is not drawn`)
  }
  // A refusal is said through errorCopy, never shown raw.
  assert.match(view, /setAdblock\([^)]*\)[\s\S]{0,200}errorCopy/)
})

test('the list chooser is a dialog by the dialog rules, with each licence beside its list', () => {
  const src = read('./BrowserFilterLists.jsx')
  assert.match(src, /useDialog\(panelRef, onClose\)/)
  assert.match(src, /useBack\(true, onClose\)/)
  assert.match(src, /errorCopy\(err\)/)
  assert.match(src, /role="dialog"/)
  assert.match(src, /list\.licence/)
  assert.doesNotMatch(src, /addEventListener\('keydown'/)
  // Nothing is ticked that the user did not tick.
  assert.match(src, /useState\(\(\) => new Set\(pickedIds\(status\)\)\)/)
})

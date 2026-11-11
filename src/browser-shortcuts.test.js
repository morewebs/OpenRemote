// The new tab page's shortcuts and dev-server row (ported from the browser
// branch's BrowserNewTab, 0d3211a, and dfcf317's dev-server row): the
// daemon's tiles, eight at most, the user's own first; "Add shortcut" while
// there is room; a shortcut is an address, never words; and the dev servers
// seen in chats, six at most. A private tab's page shows neither.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { SERVER_CAP, TILE_CAP, canAddTile, serverRows, tileLabel, tileLetter, tilesFrom } from './browser-newtab.js'
import { addressFor, bareAddress } from './browser-omnibox.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test("the tiles are the daemon's rows with an address, eight at most", () => {
  assert.equal(TILE_CAP, 8)
  assert.deepEqual(tilesFrom(null), [])
  assert.deepEqual(tilesFrom({ error: 'no' }), [])
  const got = tilesFrom([
    { url: 'https://mine.test/', title: ' Mine ', pinned: true },
    { url: '', title: 'nothing' },
    null,
    { url: 'https://news.test/' },
  ])
  assert.deepEqual(got, [
    { url: 'https://mine.test/', title: 'Mine', pinned: true },
    { url: 'https://news.test/', title: '', pinned: false },
  ])
  const many = Array.from({ length: 12 }, (_, i) => ({ url: `https://s${i}.test/` }))
  assert.equal(tilesFrom(many).length, TILE_CAP)
})

test('a tile is drawn by its name, else its site, with the letter of its site', () => {
  assert.equal(tileLabel({ url: 'https://www.example.com/a', title: 'Ex' }), 'Ex')
  assert.equal(tileLabel({ url: 'https://www.example.com/a', title: '' }), 'example.com')
  assert.equal(tileLetter({ url: 'https://www.github.com/' }), 'G')
  assert.equal(tileLetter({ url: '' }), '?')
})

test('"Add shortcut" shows while the user\'s own shortcuts have room', () => {
  const own = (n) => Array.from({ length: n }, (_, i) => ({ url: `https://p${i}.test/`, pinned: true }))
  assert.equal(canAddTile([]), true)
  assert.equal(canAddTile([...own(7), { url: 'https://v.test/', pinned: false }]), true)
  assert.equal(canAddTile(own(8)), false)
})

test('a shortcut is an address, never a search', () => {
  assert.equal(addressFor('example.com'), 'https://example.com')
  assert.equal(addressFor('localhost:5173'), 'http://localhost:5173')
  assert.equal(addressFor('https://a.test/x'), 'https://a.test/x')
  assert.equal(addressFor('how do magnets work'), null)
  assert.equal(addressFor('javascript:alert(1)'), null)
  assert.equal(addressFor('  '), null)
  assert.equal(bareAddress('https://www.Example.com/'), 'example.com/')
})

test('the dev-server row: newest first, six at most, drawn by the bare address', () => {
  assert.equal(SERVER_CAP, 6)
  const servers = Array.from({ length: 8 }, (_, i) => ({ url: `http://localhost:${3000 + i}`, chat: `c${i}` }))
  const rows = serverRows(servers)
  assert.equal(rows.length, SERVER_CAP)
  assert.deepEqual(rows[0], { url: 'http://localhost:3000', label: 'localhost:3000', chat: 'c0' })
  assert.deepEqual(serverRows(null), [])
})

test('the page reads the tiles and the dev servers, and a private tab shows neither', () => {
  const view = read('./BrowserView.jsx')
  const ntp = view.slice(view.indexOf('className="bw-ntp"'), view.indexOf('{historyOpen &&'))
  // The private note and the shortcuts are the two arms of one choice.
  assert.ok(ntp.indexOf('current?.private ?') < ntp.indexOf('<BrowserShortcuts'))
  assert.ok(ntp.indexOf('bw-ntp-private') < ntp.indexOf('<BrowserShortcuts'))
  assert.match(ntp, /<BrowserShortcuts onOpen=\{\(url\) => openUrl\(url\)\} onError=\{notify\} \/>/)
  const page = read('./BrowserShortcuts.jsx')
  assert.match(page, /browserCall\('GET', '\/browser\/tiles'\)/)
  assert.match(page, /browserCall\('POST', '\/browser\/tiles', next\)/)
  assert.match(page, /browserCall\('POST', '\/browser\/tiles\/remove', \{ url: tile\.url \}\)/)
  // recentDevServers() builds a new list on each read: kept in state, never a
  // snapshot store, which would render forever.
  assert.match(page, /subscribeDevServers\(\(\) => setServers\(recentDevServers\(\)\)\)/)
  assert.doesNotMatch(page, /useSyncExternalStore/)
  // The dialog family, and refusals in errorCopy's words.
  assert.match(page, /useDialog\(panelRef, onClose\)/)
  assert.match(page, /setError\(errorCopy\(err\)\)/)
  assert.match(page, /onError\?\.\(errorCopy\(err\)\)/)
})

// The bookmarks' rules, and the two surfaces that show them. The store keeps
// the list on this device; these tests pin the arithmetic and the seam.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import {
  FOLDERS,
  bookmarkFor,
  inFolder,
  isBookmarked,
  labelFor,
  sortBookmarks,
  starFor,
  stepPosition,
  withBookmark,
  withoutBookmark,
} from './browser-bookmarks.js'

const mark = (id, url, over = {}) => ({ id, url, title: '', folder: 'bar', position: 0, ...over })

test('the two folders are the ones Chrome has, with their own words', () => {
  assert.deepEqual(FOLDERS, { bar: 'Bookmarks bar', other: 'Other bookmarks' })
})

test('the bar comes first, then each folder in position order, ties by id', () => {
  const marks = [
    mark('d', 'https://d.example', { folder: 'other', position: 1 }),
    mark('b', 'https://b.example', { position: 2 }),
    mark('a', 'https://a.example', { position: 1 }),
    mark('c', 'https://c.example', { position: 2 }),
  ]
  assert.deepEqual(
    sortBookmarks(marks).map((m) => m.id),
    ['a', 'b', 'c', 'd'],
  )
  assert.deepEqual(inFolder(marks, 'bar').map((m) => m.id), ['a', 'b', 'c'])
  assert.deepEqual(inFolder(marks, 'other').map((m) => m.id), ['d'])
})

test('one address is one bookmark: the star edits what is already kept', () => {
  const marks = [mark('a', 'https://a.example', { title: 'A' })]
  const star = starFor(marks, { url: 'https://a.example', title: 'Again', id: 'b' })
  assert.equal(star.existing, true)
  assert.equal(star.mark.id, 'a')
  assert.equal(bookmarkFor(marks, 'https://a.example').title, 'A')
  assert.equal(isBookmarked(marks, 'https://a.example'), true)
  assert.equal(isBookmarked(marks, 'https://other.example'), false)
  // No address is no bookmark: a new tab page has nothing to keep.
  assert.equal(starFor(marks, { url: '' }), null)
})

test('a new bookmark lands at the end of its folder, and the folder is one of the two', () => {
  const marks = [mark('a', 'https://a.example', { position: 1 }), mark('b', 'https://b.example', { position: 2 })]
  const star = starFor(marks, { url: 'https://c.example', title: ' C ', id: 'c', folder: 'other' })
  assert.equal(star.existing, false)
  assert.equal(star.mark.folder, 'other')
  assert.equal(star.mark.position, 1)
  assert.equal(star.mark.title, 'C')
  // A folder this console does not know is the bar, not a third folder.
  assert.equal(starFor(marks, { url: 'https://d.example', id: 'd', folder: 'nope' }).mark.folder, 'bar')
})

test('a bookmark with no name is drawn as its address, never as an empty chip', () => {
  assert.equal(labelFor(mark('a', 'https://example.com/docs/')), 'example.com/docs')
  assert.equal(labelFor(mark('a', 'https://example.com/', { title: '  ' })), 'example.com')
  assert.equal(labelFor(mark('a', 'https://example.com', { title: ' Docs ' })), 'Docs')
})

test('adding, replacing and removing keep one row per id', () => {
  const one = withBookmark([], mark('a', 'https://a.example'))
  assert.equal(one.length, 1)
  const edited = withBookmark(one, { ...one[0], title: 'A' })
  assert.equal(edited.length, 1)
  assert.equal(edited[0].title, 'A')
  assert.deepEqual(withoutBookmark(edited, 'a'), [])
})

test('a move stops at the ends instead of leaving the folder', () => {
  const marks = [mark('a', 'https://a.example', { position: 1 }), mark('b', 'https://b.example', { position: 2 })]
  const up = stepPosition(marks, marks[0], -1)
  assert.equal(up, null)
  const down = stepPosition(marks, marks[1], 1)
  assert.equal(down, null)
  // Moving the first one down puts it between the second and nothing: after it.
  const moved = stepPosition(marks, marks[0], 1)
  assert.ok(moved > marks[1].position)
})

test('the store keeps bookmarks on this device and the view draws the three surfaces', () => {
  const store = readFileSync(new URL('./browser-store.js', import.meta.url), 'utf8')
  const view = readFileSync(new URL('./BrowserView.jsx', import.meta.url), 'utf8')
  // Per device, in the browser's own store, with no account and no API.
  assert.match(store, /export function saveBookmark\(/)
  assert.match(store, /export function removeBookmark\(/)
  assert.match(store, /localStorage\.setItem\(BOOKMARKS_KEY/)
  assert.doesNotMatch(store, /browser_(bookmark|sync)/)
  // The star sits beside the address bar, the bar under the tab strip, the
  // editor is the dialog family, and Ctrl+D is the pane's own key.
  assert.match(view, /data-bw-anchor="star"/)
  assert.match(view, /<BrowserBookmarksBar/)
  assert.match(view, /<BrowserBookmarkEditor/)
  // Ctrl+D is the catalog's browser-bookmark key (shortcuts.js), run as star.
  assert.match(view, /'browser-bookmark': star/)
  const shortcuts = readFileSync(new URL('./shortcuts.js', import.meta.url), 'utf8')
  assert.match(shortcuts, /id: 'browser-bookmark', group: 'browser', label: 'Bookmark this tab', keys: 'Mod\+D'/)
})

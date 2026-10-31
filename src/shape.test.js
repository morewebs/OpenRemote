// The three coercions every daemon-shaped value goes through, and the one
// property that matters most: an encoded id cannot leave the path segment it
// was meant for, while a raw one can. The second half of that pair is the
// hazard this file exists to prevent, so it is asserted too.
//
// The last test is the one a second reader would not expect here: it scans the
// surfaces for the fallback fed by hand, because `chatTitle` exists to stop the
// console having eight copies of one sentence.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { CHAT_TITLE_FALLBACK, chatTitle, list, seg, text } from './shape.js'

test('an encoded id stays in its segment, where a raw one changes the route', () => {
  const hostile = '../automations/abc'
  // The hazard: a chat's delete aimed at a rule's delete.
  assert.equal(new URL(`http://daemon/sessions/${hostile}`).pathname, '/automations/abc')
  // The fix: the same id is one segment, so the route cannot move.
  assert.equal(
    new URL(`http://daemon/sessions/${seg(hostile)}`).pathname,
    '/sessions/..%2Fautomations%2Fabc',
  )
  // A plugin id is a name and a marketplace, and the @ survives as data.
  assert.equal(seg('clash@local'), 'clash%40local')
  assert.equal(new URL(`http://daemon/plugins/${seg('a/../b')}/enabled`).pathname, '/plugins/a%2F..%2Fb/enabled')
  // A missing id is a wiring bug, not an empty segment: `DELETE /sessions/`
  // answers 404 and this client reads that as "already gone".
  assert.throws(() => seg(null), /needs an id/)
  assert.throws(() => seg(undefined), /needs an id/)
  assert.throws(() => seg(''), /needs an id/)
  assert.equal(seg(7), '7')
  assert.equal(seg(0), '0', 'a zero id is still an id')
})

test('text gives a string for anything, and empty for a shape that has none', () => {
  assert.equal(text('claude'), 'claude')
  assert.equal(text(7), '7')
  assert.equal(text(true), 'true')
  assert.equal(text(null), '')
  assert.equal(text(undefined), '')
  // An object is not "[object Object]" on screen: it is nothing.
  assert.equal(text({ name: 'x' }), '')
  assert.equal(text(['a']), '')
  // The point of the coercion: a name from the daemon can go to localeCompare.
  assert.doesNotThrow(() => text({}).localeCompare('x'))
})

test('list gives an array for anything, and empty for a shape that is not one', () => {
  assert.deepEqual(list([1, 2]), [1, 2])
  assert.deepEqual(list(null), [])
  assert.deepEqual(list(undefined), [])
  assert.deepEqual(list({}), [])
  assert.deepEqual(list('three'), [])
  // A malformed answer behaves like a server with nothing to say.
  assert.deepEqual(list({}).filter(() => true), [])
})

test('a chat name is its title, trimmed, or the one fallback', () => {
  assert.equal(chatTitle({ title: 'fix the parser' }), 'fix the parser')
  assert.equal(chatTitle({ title: '  fix the parser  ' }), 'fix the parser', 'edges trimmed')
  assert.equal(chatTitle({}), CHAT_TITLE_FALLBACK)
  assert.equal(chatTitle({ title: null }), CHAT_TITLE_FALLBACK)
  assert.equal(chatTitle({ title: undefined }), CHAT_TITLE_FALLBACK)
  assert.equal(chatTitle(null), CHAT_TITLE_FALLBACK)
  // The corners the eight hand-written copies disagreed on:
  assert.equal(chatTitle({ title: '' }), CHAT_TITLE_FALLBACK, 'an empty title is no title')
  assert.equal(chatTitle({ title: '   ' }), CHAT_TITLE_FALLBACK, 'whitespace is no title either')
  assert.equal(chatTitle({ title: 42 }), '42', 'a numeric title still reads, as text')
  assert.equal(chatTitle({ title: {} }), CHAT_TITLE_FALLBACK, 'an object reads as no title, not a crash')
  assert.equal(CHAT_TITLE_FALLBACK, 'A new task', 'the words every surface prints')
})

test('no surface feeds the chat-title fallback by hand any more', () => {
  // The surfaces that print a chat's name. Each one used to write the sentence
  // itself; if a new copy appears, this is where it shows up. New chat is not
  // listed: it hands the chat it just made straight to its pane and prints no
  // chat name of its own. ChatView.jsx and stop-everything.js are not listed
  // because those two files are not this change's to edit; their lines are
  // named in the hand-off note instead.
  const surfaces = [
    'Sidebar.jsx',
    'CommandPalette.jsx',
    'App.jsx',
    'SettingsView.jsx',
    'chat-markdown.js',
    'settings-search.js',
  ]
  for (const file of surfaces) {
    const source = readFileSync(new URL(file, import.meta.url), 'utf8')
    assert.doesNotMatch(
      source,
      /\?\?\s*'A new task'|\|\|\s*'A new task'/,
      `${file} still writes the fallback by hand`,
    )
    assert.match(source, /chatTitle/, `${file} prints the name through chatTitle`)
  }
})
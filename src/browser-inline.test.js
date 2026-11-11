// The address bar's inline completion (ported from the browser branch's
// BrowserOmnibox, 0d3211a): a typed beginning is completed in the field from
// the suggestions - the site, or the whole address once the typing reaches
// past it - selected so the next key replaces it; Tab or Right takes it.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { completion, inlineFor, suggest } from './browser-omnibox.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('a typed beginning completes to the site, then to the whole address', () => {
  assert.equal(completion('git', 'https://github.com/openremote/app'), 'github.com')
  assert.equal(completion('www.git', 'https://www.github.com/'), 'www.github.com')
  assert.equal(completion('github.com/op', 'https://github.com/openremote/app'), 'github.com/openremote/app')
  // What was typed keeps its own letters; only the rest is added.
  assert.equal(completion('GIT', 'https://github.com/'), 'GIThub.com')
})

test('words, a scheme, a mismatch or a finished address are never completed', () => {
  assert.equal(completion('how do', 'https://howdo.com/'), null)
  assert.equal(completion('https://git', 'https://github.com/'), null)
  assert.equal(completion('gitlab', 'https://github.com/'), null)
  assert.equal(completion('github.com', 'https://github.com/'), null)
  assert.equal(completion('', 'https://github.com/'), null)
})

test('the first suggestion that completes is the one shown', () => {
  const marks = [{ url: 'https://news.ycombinator.com/', title: 'Hacker News' }]
  const visits = [{ url: 'https://nature.com/articles', title: 'Nature', at: 2 }]
  const rows = suggest({ typed: 'na', marks, visits })
  assert.equal(inlineFor('na', rows), 'nature.com')
  assert.equal(inlineFor('ne', suggest({ typed: 'ne', marks, visits })), 'news.ycombinator.com')
  assert.equal(inlineFor('zz', rows), null)
  assert.equal(inlineFor('na', null), null)
})

test('the view completes only typing forward at the end, selects the rest, and Tab or Right takes it', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /aria-autocomplete="both"/)
  assert.match(view, /inputType\?\.startsWith\('insert'\)\) && e\.target\.selectionStart === next\.length/)
  assert.match(view, /inlineFor\(next, suggest\(\{ typed: next, marks: bookmarks, visits: history \}\)\)/)
  assert.match(view, /completedRange\.current = \[next\.length, inline\.length\]/)
  assert.match(view, /addressRef\.current\.setSelectionRange\(range\[0\], range\[1\]\)/)
  assert.match(view, /\(e\.key === 'Tab' && !e\.shiftKey\) \|\| e\.key === 'ArrowRight'/)
  // The suggestions follow what was typed, not the completed field.
  assert.match(view, /const typedLine = editing && typed && typed !== shownAddress\(current\?\.url\) \? typed : ''/)
})

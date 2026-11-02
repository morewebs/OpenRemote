// The Installed tab's search, wired: one field that reuses the Discover search
// itself, a list drawn from the matched set, the count only while filtering,
// and the two empty answers kept apart. The matcher is plugins.js's own and is
// pinned in plugins.test.js.
//
// plugins-wiring.test.js was dirty in another lane's hands when this was
// written, so this file is separate rather than an edit to it.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')
const view = source('PluginsView.jsx')
const css = source('plugins.css')

test('one search field over the installed list, and only when there is a list', () => {
  assert.match(view, /import \{[\s\S]{0,260}installedSearchHint,/)
  assert.match(view, /const shown = searchInstalled\(installed, query\)/)
  assert.match(view, /const filtering = query\.trim\(\)\.length > 0/)
  assert.match(view, /\{installed\.length > 0 && \(\s*<div className="pg-installed-toolbar">/)
  // The same field the Discover tab uses, not a second field style.
  assert.match(view, /className="pg-search-box"/)
  assert.match(view, /className="pg-search-input"/)
  assert.match(view, /className="pg-search-icon"/)
  assert.match(view, /className="pg-search-clear"/)
  assert.match(view, /aria-label="Search installed plugins"/)
})

test('the hint comes from the rule, and Escape clears then leaves', () => {
  assert.match(view, /placeholder=\{installedSearchHint\(installed\.length\)\}/)
  assert.doesNotMatch(view, /placeholder="Search installed plugins"/)
  assert.match(view, /if \(e\.key !== 'Escape'\) return/)
  assert.match(view, /if \(query\) setQuery\(''\)/)
  assert.match(view, /else e\.currentTarget\.blur\(\)/)
})

test('the list draws what the query matched, and the built-in section is untouched', () => {
  assert.match(view, /\{shown\.map\(\(p\) => \{/)
  assert.doesNotMatch(view, /\{installed\.map\(\(p\) => \{/)
  // The six built-in harness rows keep their own section and its switch.
  assert.match(view, /<h2 className="pn-section">Built in<\/h2>/)
  assert.match(view, /Off hides an app from every picker\. Its chats keep working\./)
})

test('the count appears while filtering, and the two empty answers stay apart', () => {
  assert.match(view, /filtering && shown\.length > 0 && \(/)
  assert.match(view, /installedCountLine\(shown\.length, installed\.length\)/)
  // Nothing installed: the module's invitation, not a search result.
  assert.match(view, /\{installed\.length === 0 && <p className="pn-empty">\{INSTALLED_EMPTY_NONE\}<\/p>\}/)
  // A query that found nothing: its own sentence, naming the query.
  assert.match(view, /filtering && installed\.length > 0 && shown\.length === 0/)
  assert.match(view, /installedEmptyLine\(query\)/)
})

test('the toolbar has its own row in the sheet, and adds no second field style', () => {
  assert.match(css, /\.pg-installed-toolbar \{/)
  assert.match(css, /\.pg-installed-count \{/)
  // The field's look is the Discover search's, defined once.
  const fields = css.match(/\.pg-search-input \{/g) ?? []
  assert.equal(fields.length, 1, 'one search-input rule for both tabs')
})

test('the install door and the panel behind it use the same words', () => {
  // The header names what the dialog is, and the dialog is titled the same:
  // its three sources are its own tabs, not a list in the button.
  assert.match(view, />\s*Install a plugin\s*</)
  assert.doesNotMatch(view, /Install from a file or link/)
  const modal = source('InstallPluginModal.jsx')
  assert.match(modal, /: 'Install a plugin'/)
  for (const label of ['Folder', 'Git repo', 'Archive']) {
    assert.match(modal, new RegExp(`label: '${label}'`), `the dialog names its ${label} source`)
  }
})

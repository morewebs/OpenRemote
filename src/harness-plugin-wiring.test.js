// The plugin-harness surface on the console: a monogram instead of a borrowed
// brand mark, a "Harness" chip on the plugin that provides one, and the
// daemon's own name printed wherever a harness is named. Most of it is JSX and
// CSS, and this suite has no renderer (see plugins-wiring.test.js), so the
// wiring is pinned as source text here. The label rule itself is unit tested
// in harness-names.test.js.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('an unknown harness id gets a quiet monogram, never another owner\'s mark', () => {
  const marks = source('brand-marks.jsx')
  // No id is left with nothing to render, which is what a plugin harness met.
  assert.doesNotMatch(marks, /if \(!mark\) return null/)
  assert.match(marks, /function MonogramMark\(/)
  assert.match(marks, /brand-mark-mono/)
  // The letter is the id's own first character, so the tile names the
  // harness instead of borrowing a logo.
  assert.match(marks, /slice\(0, 1\)\.toUpperCase\(\)/)
  // A harness id is an arbitrary word: the registry is asked with a guard, so
  // "constructor" cannot resolve to Object's own member and crash the render.
  assert.match(marks, /Object\.hasOwn\(MARKS, harness\)/)
  // The tile's look is the plugin icons' own grey, at the mark's size.
  assert.match(
    source('composer.css'),
    /\.brand-mark-mono \{[\s\S]{0,240}rgba\(255, 255, 255, 0\.05\)/,
  )
})

test('the installed list marks a plugin that provides a harness, and stays quiet without one', () => {
  const view = source('PluginsView.jsx')
  assert.match(view, /\{p\.harness && <span className="pg-badge-harness">App<\/span>\}/)
  // The chip takes the badge's existing styling; it adds no accent of its own.
  assert.match(source('plugins.css'), /\.pg-badge-marketplace,\s*\n\.pg-badge-harness \{/)
})

test('every surface that names a harness can print the daemon\'s own name', () => {
  // The manager's rows carry a whole harness object, so the label needs no
  // second source there.
  assert.match(source('harness-manager.js'), /const name = harnessLabel\(h\)/)
  // New chat holds the harness it picked, and the full list for a preset,
  // which records only an id.
  const newChat = source('NewChat.jsx')
  assert.match(newChat, /harnessLabel\(currentHarness\)/)
  assert.match(newChat, /harnessLabel\(preset\.harness, harnessList\)/)
  // The copy's speaker takes the list from the caller that has it.
  assert.match(source('chat-markdown.js'), /harnessLabel\(harness \?\? chat\?\.harness \?\? null, harnesses\)/)
  // The rules list prints the same label its search matches.
  assert.match(source('automation.js'), /harnessLabel\(rule\?\.harness, harnesses\)/)
  assert.match(source('AutomationsView.jsx'), /harnessLabel\(rule\.harness, harnesses\)/)
  // A chat's own header, facts row and permission tooltips all name the harness
  // through one value read from the daemon's row.
  assert.match(source('ChatView.jsx'), /const harnessText = harnessLabel\(harness \?\? chat\.harness\)/)
})

// The row and the seam it opens: the palette offers "New chat with this setup"
// only where the rule behind it can name a harness the console offers, App
// builds the seed with that same rule and hands it to New chat, and New chat
// settles it through the one path a saved preset already uses.
//
// Source text, like the repo's other wiring checks: this is the half a pure
// test cannot see, and a call site that loses its import is still valid text.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(file, import.meta.url), 'utf8')

test('the palette offers the row only where the setup rule can deliver it', () => {
  const palette = read('CommandPalette.jsx')
  assert.match(palette, /import \{ canSeedChat, chatSetup \} from '\.\/chat-setup\.js'/)
  assert.match(palette, /const rowSetup = useMemo\(/)
  assert.match(palette, /\[setupFields, capabilities, projects, thisDevice\],/)
  assert.match(
    palette,
    /chatSetup\(setupRecord, \{\s*harnesses: \(capabilities\?\.harnesses \?\? \[\]\)\.filter\(pickable\),\s*projects: projects \?\? \[\],\s*thisDevice,?\s*\}\)/,
  )
  // The row itself: the same shape as the rename row beside it.
  assert.match(palette, /key: 'action:new-with-setup'/)
  assert.match(palette, /name: 'New chat with this setup'/)
  assert.match(palette, /section: 'Actions'/)
  assert.match(palette, /run: \{ newWithSetup: focusedChatId \}/)
  assert.match(palette, /canSeedChat\(rowSetup\)/)
  // The title rides the sub line, so it is veiled with the other titles.
  assert.match(palette, /subVeiled: true/)
  // Pressing it goes through the shell, which owns navigation and the seed.
  assert.match(palette, /else if \(r\.newWithSetup\) onNewWithSetup\?\.\(r\.newWithSetup\)/)
})

test('the shell builds the seed with the same rule and hands it to New chat', () => {
  const app = read('App.jsx')
  assert.match(app, /import \{ canSeedChat, chatSetup \} from '\.\/chat-setup\.js'/)
  assert.match(app, /const newChatWithSetup = \(chatId\) => \{/)
  assert.match(app, /chatSetup\(record, \{\s*harnesses: \(capabilities\?\.harnesses \?\? \[\]\)\.filter\(pickable\),\s*projects: projects \?\? \[\],\s*thisDevice,?\s*\}\)/)
  assert.match(app, /if \(!canSeedChat\(setup\)\) return/)
  assert.match(app, /setSetupSeed\(\{ nonce: Date\.now\(\), \.\.\.setup, leafId:/)
  assert.match(app, /setupSeed=\{setupSeed\?\.leafId === leaf\.id \? setupSeed : undefined\}/, 'promotion never inherits another pane\'s setup')
  assert.match(app, /onNewWithSetup=\{newChatWithSetup\}/)
  assert.match(app, /setupSeed=\{setupSeed\}/)
})

test('New chat settles a seed through the preset path rather than a second one', () => {
  const view = read('NewChat.jsx')
  assert.match(view, /setupSeed = null,/)
  assert.match(view, /pendingPreset\.current = setupSeed/)
  assert.match(view, /if \(setupSeed\.harness\) setHarness\(setupSeed\.harness\)/)
  // The model list effect has to run again for a seed, or a same-harness seed
  // would wait for a fetch that never comes.
  assert.match(view, /\}, \[currentHarness\?\.id, connection\.state, target, setupSeed\?\.nonce\]\)/)
})

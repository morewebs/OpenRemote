// The composer, chat details and saved presets all use the App's own
// effort word so one tier does not have two names.
//
// Source text, like the repo's other wiring checks: this is the half a pure
// test cannot see, because NewChat.jsx cannot be imported without a browser.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the effort chip prints the App tier, not a name OpenRemote made up', () => {
  const view = read('NewChat.jsx')
  // The mapping table and the title-casing fallback are both gone.
  assert.doesNotMatch(view, /EFFORT_LABELS/)
  assert.doesNotMatch(view, /'Extra High'/)
  assert.doesNotMatch(view, /toUpperCase\(\)/)
  // What is left passes the tier through, and names the empty case.
  assert.match(
    view,
    /export function effortLabel\(tier\) \{\s*\n\s*if \(!tier\) return 'Effort'\s*\n\s*return tier\s*\n\}/,
  )
  // The tiers offered are the catalog's own strings, untouched: the slider
  // shows what the App accepts rather than a parallel set of labels.
  assert.match(view, /const effortTiers = activeModel\?\.reasoning_efforts \?\? \[\]/)
})

test('the chip and the status dialog agree on the word', () => {
  const view = read('ChatView.jsx')
  assert.match(view, /<StatusModal chat=\{chat\}/)
  assert.match(read('SlashDialogs.jsx'), /const rows = statusRows\(chat, /)
  assert.match(read('slash-actions.js'), /rows\.push\(\['Effort', chat\.effort\]\)/)
  assert.match(view, /<EffortLabel text=\{effortLabel\(chat\.effort\)\} \/>/)
})

test('both composers put the effort control after the model and before the provider or App', () => {
  for (const [file, appHook] of [['NewChat.jsx', '{harnessPicker}'], ['ChatView.jsx', 'data-picker="harness"']]) {
    const row = read(file).split('<div className="nc-pickers">')[1]
    assert.ok(row, file)
    const positions = ['data-picker="model"', 'data-picker="effort"', 'className="nc-via"', appHook]
      .map(hook => row.indexOf(hook))
    assert.ok(positions.every((position, index) => position >= 0 && (!index || position > positions[index - 1])), file)
    assert.ok(row.indexOf('data-picker="endpoint"') > positions[1], file)
  }
})

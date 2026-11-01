// An effort tier is the harness's own word for how hard it should think.
// The composer chip used to translate it - `xhigh` became "Extra High",
// `maximum` became "Max" - while the facts row above the transcript and a
// saved preset both printed the raw tier, so one tier had two names and one
// of them was a word no harness ships. The chip prints the harness's word
// now, and there is no table left that could disagree with it.
//
// Source text, like the repo's other wiring checks: this is the half a pure
// test cannot see, because NewChat.jsx cannot be imported without a browser.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the effort chip prints the harness tier, not a name this app made up', () => {
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
  // shows what the harness accepts rather than a parallel set of labels.
  assert.match(view, /const effortTiers = activeModel\?\.reasoning_efforts \?\? \[\]/)
})

test('the chip and the facts row agree on the word', () => {
  const view = read('ChatView.jsx')
  // The facts row has always printed the raw tier; the chip now matches it
  // rather than naming the same tier twice.
  assert.match(view, /\{chat\.effort && <span className="cv-fact">\{chat\.effort\}<\/span>\}/)
  assert.match(view, /<FadingLabel text=\{effortLabel\(chat\.effort\)\} \/>/)
})

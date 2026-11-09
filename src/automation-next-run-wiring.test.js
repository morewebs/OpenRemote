// The rules row's next-run fragment, wired: the helper decides the words,
// and the row prints them after the last-run stamp. Source text, like the
// repo's other wiring checks.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the rules row states when the rule will next fire', () => {
  const view = read('AutomationsView.jsx')
  assert.match(view, /import \{ nextRunLabel \} from '\.\/next-run\.js'/)
  assert.match(view, /const nextRun = nextRunLabel\(rule\)/)
  assert.match(view, /ranLabel\(rule\)/)
  assert.match(view, /\{nextRun \? ` · \$\{nextRun\}` : ''\}/)
})

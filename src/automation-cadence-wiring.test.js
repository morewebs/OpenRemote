// The rule editor's cadence picker, wired: a daily time or an interval, never
// both, and the trigger body is built by the one helper the vocabulary tests.
// Source text, like the repo's other wiring checks.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the rule editor offers a cadence beside the daily time', () => {
  const form = read('AutomationForm.jsx')
  assert.match(form, /every_minutes/)
  assert.match(form, /picker\?\.kind === 'cadence'/)
})

test('the rule body is built by the cadence helper', () => {
  const form = read('AutomationForm.jsx')
  assert.match(form, /scheduleTrigger\(cadence, time\.trim\(\)\)/)
})

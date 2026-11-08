// The rule editor's effort picker, wired: the tiers are the picked model's
// own catalog strings, and the word rides the save body only when that model
// still offers it. Source text, like the repo's other wiring checks.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test("the rule editor offers the picked model's own effort tiers", () => {
  const form = read('AutomationForm.jsx')
  assert.match(form, /reasoning_efforts/)
  assert.match(form, /picker\?\.kind === 'effort'/)
  assert.match(form, /chosenEffort\(/)
})

test('the rule body carries effort to the daemon', () => {
  const api = read('api/daemon.js')
  assert.match(api, /\.\.\.\(rule\.effort \? \{ effort: rule\.effort \} : \{\}\)/)
})

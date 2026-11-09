// The web sign-in surface's callback, wired: the page that comes back from
// the gateway reads and clears only the state it was given, and the sign-in
// button cannot start a second sign-in over the first. Source text, like the
// repo's other wiring checks.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const view = () => readFileSync(new URL('./SignInSurface.jsx', import.meta.url), 'utf8')

test('a callback reads and clears only the state it came back with', () => {
  const source = view()
  assert.match(source, /const pending = loadPending\(state\)/)
  assert.match(source, /clearPending\(state\)/)
  assert.doesNotMatch(source, /const pending = loadPending\(\)/)
})

test('the sign-in button cannot start two sign-ins', () => {
  const source = view()
  assert.match(source, /if \(starting\) return/)
  assert.match(source, /disabled=\{starting\}/)
  assert.match(source, /const saved = savePending\(/)
  assert.match(source, /if \(!saved\)/)
})

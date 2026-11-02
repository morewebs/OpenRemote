// The survival rule for a harness switch: a word the new harness lists, a
// model its catalog actually carries, and one sentence for whatever did not
// come along. The permission words in these tests are the daemon's own (its
// registry): claude offers default / acceptEdits / bypassPermissions / plan,
// grok offers those four plus auto and dontAsk, codex uses untrusted /
// on-failure / on-request / never, and the other three offer none.
//
// The list arrives as rows, `{ w, l }` with the flag in `w` and the label in
// `l` (openremote-core's PermissionWord). These tests pass that shape, which
// is the one the console actually hands over - a string-only test passed while
// the real list compared nothing, which is how the permission half of the
// carry came to never carry.

import assert from 'node:assert/strict'
import test from 'node:test'
import { carrySetup, permissionWords, restoreFromCatalog } from './harness-carry.js'

const rows = (words) => words.map((w) => ({ w, l: `Label for ${w}` }))

const CLAUDE_WORDS = rows(['default', 'acceptEdits', 'bypassPermissions', 'plan'])
const GROK_WORDS = rows(['default', 'acceptEdits', 'auto', 'dontAsk', 'bypassPermissions', 'plan'])
const CODEX_WORDS = rows(['untrusted', 'on-failure', 'on-request', 'never'])

const models = [
  { model: 'opus', display_name: 'Opus 5.5', resolved_model: 'claude-opus-5-5' },
  { model: 'sonnet', display_name: 'Sonnet 5', resolved_model: 'claude-sonnet-5' },
]

test('a model survives only where the new catalog lists it', () => {
  const kept = carrySetup({ model: 'opus' }, { models, permissionModes: [] })
  assert.equal(kept.kept.model, 'opus')
  // The same harness's catalog carries it by pick word or by resolved id.
  assert.equal(carrySetup({ model: 'claude-opus-5-5' }, { models, permissionModes: [] }).kept.model, 'claude-opus-5-5')
  // Another maker's catalog has no such row: dropped, not quietly switched.
  const dropped = carrySetup({ model: 'opus' }, { models: [], permissionModes: [] })
  assert.equal(dropped.kept.model, null)
  assert.deepEqual(dropped.dropped, ['model'])
})

test('a model with no catalog to check against yet is dropped, and can come back', () => {
  // The new harness's list is still in flight: an unknown catalog carries
  // nothing, and the answer puts it back if the row is there.
  const dropped = carrySetup({ model: 'opus' }, { models: null, permissionModes: [] })
  assert.deepEqual(dropped.dropped, ['model'])
  assert.equal(restoreFromCatalog('opus', models), 'opus')
  assert.equal(restoreFromCatalog('claude-opus-5-5', models), 'claude-opus-5-5')
  assert.equal(restoreFromCatalog('gpt-5', models), null)
  assert.equal(restoreFromCatalog(null, models), null)
})

test('the words are read out of the daemon rows, whatever shape they arrive in', () => {
  // The wire shape: rows with the flag in `w`. This is what the console hands
  // over, and reading `includes` on the list instead of the words in it is the
  // bug this pins: a string compared against objects is always false.
  assert.deepEqual(permissionWords(GROK_WORDS), ['default', 'acceptEdits', 'auto', 'dontAsk', 'bypassPermissions', 'plan'])
  // A hand-built list of plain strings still works.
  assert.deepEqual(permissionWords(['default', 'plan']), ['default', 'plan'])
  assert.deepEqual(permissionWords([]), [])
  assert.deepEqual(permissionWords(null), [])
  // A malformed row is skipped, not turned into the word "undefined".
  assert.deepEqual(permissionWords([{ l: 'no flag' }, { w: 'plan', l: 'Plan only' }]), ['plan'])
})

test('a permission word survives exactly where the harness speaks it', () => {
  // claude -> grok: every claude word is one of grok's six, rows or not.
  for (const mode of CLAUDE_WORDS) {
    const out = carrySetup({ permission: mode.w }, { models: [], permissionModes: GROK_WORDS })
    assert.equal(out.kept.permission, mode.w, `${mode.w} carries to grok`)
    assert.deepEqual(out.dropped, [])
  }
  // grok -> claude: two of grok's words are its own.
  for (const word of ['auto', 'dontAsk']) {
    const out = carrySetup({ permission: word }, { models: [], permissionModes: CLAUDE_WORDS })
    assert.equal(out.kept.permission, null)
    assert.deepEqual(out.dropped, ['permission'])
  }
  // claude -> codex: no word is shared, in either direction.
  assert.deepEqual(carrySetup({ permission: 'plan' }, { models: [], permissionModes: CODEX_WORDS }).dropped, ['permission'])
  // Any harness with no words at all cannot carry one.
  assert.deepEqual(carrySetup({ permission: 'plan' }, { models: [], permissionModes: [] }).dropped, ['permission'])
  // And the same verdict for the plain-string shape a fixture might use.
  assert.equal(carrySetup({ permission: 'plan' }, { models: [], permissionModes: ['default', 'plan'] }).kept.permission, 'plan')
})

test('both fields are dropped together', () => {
  const out = carrySetup({ model: 'opus', permission: 'plan' }, { models: [], permissionModes: CODEX_WORDS })
  assert.deepEqual(out.dropped, ['model', 'permission'])
})

test('nothing set, or everything carried, says nothing', () => {
  assert.deepEqual(carrySetup({}, { models, permissionModes: CLAUDE_WORDS }), { kept: { model: null, permission: null }, dropped: [] })
  const carried = carrySetup({ model: 'opus', permission: 'plan' }, { models, permissionModes: CLAUDE_WORDS })
  assert.deepEqual(carried.dropped, [])
  assert.equal(carried.kept.model, 'opus')
  assert.equal(carried.kept.permission, 'plan')
})
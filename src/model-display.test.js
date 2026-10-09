import { test } from 'node:test'
import assert from 'node:assert/strict'
import { findModel, modelDisplayName } from './model-display.js'

// The claude catalog shape the daemon serves (observed wire, folded):
// pick words, resolved ids, and display names that carry the version.
const CLAUDE = [
  { model: 'default', resolved_model: 'claude-opus-5-5', display_name: 'Default (recommended)', is_default: true },
  { model: 'opus', resolved_model: 'claude-opus-5-5', display_name: 'Opus 5.5', is_default: false },
  { model: 'claude-fable-5-1[1m]', resolved_model: 'claude-fable-5-1[1m]', display_name: 'Fable 5.1', is_default: false },
  { model: 'sonnet', resolved_model: 'claude-sonnet-5-5', display_name: 'Sonnet 5.5', is_default: false },
]

test('a pick word finds its row and its display name', () => {
  assert.equal(modelDisplayName(CLAUDE, 'opus'), 'Opus 5.5')
  assert.equal(modelDisplayName(CLAUDE, 'sonnet'), 'Sonnet 5.5')
})

test('the resolved id a running session reports finds the same row', () => {
  // The first turn's init frame replaces the pick word with the id the
  // harness actually runs - the chip must not change its name.
  assert.equal(modelDisplayName(CLAUDE, 'claude-opus-5-5'), 'Opus 5.5')
  assert.equal(findModel(CLAUDE, 'claude-opus-5-5')?.model, 'opus')
})

test('a full id that is its own pick word matches directly', () => {
  assert.equal(modelDisplayName(CLAUDE, 'claude-fable-5-1[1m]'), 'Fable 5.1')
})

test('the default alias shows the harness recommended marker', () => {
  assert.equal(modelDisplayName(CLAUDE, 'default'), 'Default (recommended)')
})

test('an unknown id shows verbatim, never hidden or re-cased', () => {
  assert.equal(modelDisplayName(CLAUDE, 'grok-4-fast'), 'grok-4-fast')
  assert.equal(modelDisplayName([], 'opus'), 'opus')
  assert.equal(modelDisplayName(null, 'opus'), 'opus')
})

test('a catalog without resolved ids still matches by pick word', () => {
  // codex/grok/agy rows carry no resolved id - the pick word is the id.
  const codex = [{ model: 'g-6-codex', display_name: 'GPT-6-Codex' }]
  assert.equal(modelDisplayName(codex, 'g-6-codex'), 'GPT-6-Codex')
  assert.equal(findModel(codex, 'g-6-codex')?.display_name, 'GPT-6-Codex')
})

test('a row without a display name falls back to its id verbatim', () => {
  const bare = [{ model: 'grok-4-fast' }]
  assert.equal(modelDisplayName(bare, 'grok-4-fast'), 'grok-4-fast')
})

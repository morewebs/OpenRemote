// What the console calls a harness. The six built-in spellings are the
// product's own and cannot move; a harness a plugin provides is named by the
// daemon, which is the only side that knows it. Pure JS on purpose (no JSX
// import), so the rule is tested rather than described; the surfaces that
// print it are pinned as source text in harness-plugin-wiring.test.js.

import assert from 'node:assert/strict'
import test from 'node:test'

const { harnessLabel, harnessName } = await import('./harness-names.js')

const BUILT_IN = {
  claude: 'Claude Code',
  codex: 'Codex',
  grok: 'Grok Build',
  pi: 'Pi Agent',
  opencode: 'OpenCode',
  agy: 'Antigravity',
}

test('the six built-in names are exactly the product spellings', () => {
  for (const [id, name] of Object.entries(BUILT_IN)) {
    assert.equal(harnessName(id), name, id)
    assert.equal(harnessLabel(id), name, id)
  }
})

test('a built-in keeps its own name even when the daemon words it differently', () => {
  // A plugin cannot take a built-in id, so the daemon's name for one is only
  // ever the same harness. The map stays first so a daemon wording change
  // cannot silently rename the six.
  assert.equal(harnessLabel({ id: 'claude', name: 'Claude' }), 'Claude Code')
  assert.equal(harnessLabel('codex', [{ id: 'codex', name: 'Codex CLI' }]), 'Codex')
})

test('a harness this build has no name for is named by the daemon', () => {
  assert.equal(harnessLabel({ id: 'freebuff', name: 'Freebuff' }), 'Freebuff')
  // The same name, reached from an id alone and the daemon's harness list.
  assert.equal(harnessLabel('freebuff', [{ id: 'freebuff', name: 'Freebuff' }]), 'Freebuff')
})

test('the daemon name is trimmed, and a blank one says nothing', () => {
  assert.equal(harnessLabel({ id: 'freebuff', name: '  Freebuff  ' }), 'Freebuff')
  assert.equal(harnessLabel({ id: 'freebuff', name: '   ' }), 'freebuff')
  assert.equal(harnessLabel({ id: 'freebuff', name: 7 }), 'freebuff')
})

test('with no name to be had, an unknown id prints as it came', () => {
  assert.equal(harnessLabel('freebuff'), 'freebuff')
  assert.equal(harnessLabel({ id: 'freebuff' }), 'freebuff')
  // A list that does not hold this harness names nothing either.
  assert.equal(harnessLabel('freebuff', [{ id: 'claude', name: 'Claude Code' }]), 'freebuff')
  assert.equal(harnessLabel('freebuff', null), 'freebuff')
})

test('nothing at all has no label, so the caller can say its own words', () => {
  assert.equal(harnessLabel(null), null)
  assert.equal(harnessLabel(undefined), null)
  assert.equal(harnessLabel({}), null)
})

import assert from 'node:assert/strict'
import test from 'node:test'

const store = new Map()
globalThis.localStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k),
  clear: () => store.clear(),
}

const { loadDefaults, saveDefaults } = await import('./defaults.js')

test('defaults round-trip harness, model, and workspace', () => {
  localStorage.clear()
  saveDefaults({ harness: 'claude', model: 'opus', workspace: 'C:\\proj' })
  const d = loadDefaults()
  assert.equal(d.harness, 'claude')
  assert.equal(d.model, 'opus')
  assert.equal(d.workspace, 'C:\\proj')
})

test('a partial save keeps the untouched fields', () => {
  localStorage.clear()
  saveDefaults({ harness: 'codex', model: 'o4' })
  saveDefaults({ workspace: 'C:\\other' })
  const d = loadDefaults()
  assert.equal(d.harness, 'codex')
  assert.equal(d.model, 'o4')
  assert.equal(d.workspace, 'C:\\other')
})

test('the per-harness effort map survives partial saves', () => {
  localStorage.clear()
  saveDefaults({ efforts: { claude: 'high', codex: 'high' } })
  saveDefaults({ model: 'opus' })
  const d = loadDefaults()
  assert.deepEqual(d.efforts, { claude: 'high', codex: 'high' })
  // Writing one harness's tier keeps the other's.
  saveDefaults({ efforts: { ...d.efforts, claude: 'low' } })
  assert.deepEqual(loadDefaults().efforts, { claude: 'low', codex: 'high' })
})

test('garbage storage falls back to empty, non-string efforts dropped', () => {
  localStorage.clear()
  localStorage.setItem('openremote-defaults', 'nope{')
  const d = loadDefaults()
  assert.equal(d.harness, null)
  assert.deepEqual(d.efforts, {})
  localStorage.setItem(
    'openremote-defaults',
    JSON.stringify({ efforts: { claude: 'high', bad: 3, 'x y': null } }),
  )
  assert.deepEqual(loadDefaults().efforts, { claude: 'high' })
})

import assert from 'node:assert/strict'
import test from 'node:test'

const { harnessRows, capabilityWords } = await import('./harness-manager.js')

const cap = (harnesses) => ({ harnesses })
const spec = (harnessId, extra = {}) => ({
  harness_id: harnessId,
  name: harnessId,
  command: `curl -fsSL https://example.test/${harnessId} | bash`,
  source: 'official',
  needs_runtime: false,
  ...extra,
})

test('a harness advertises what it honors, and a missing answer stays unknown', () => {
  const caps = capabilityWords({
    honors: { effort: true, fast: false, mcp: true, plugins: false, tools: true, attachments: false },
  })
  assert.deepEqual(
    caps.map((cap) => [cap.key, cap.on]),
    [
      ['effort', true],
      ['fast', false],
      ['mcp', true],
      ['plugins', false],
      ['tools', true],
      ['attachments', false],
    ],
  )
  // An older daemon that did not send the object has not said "false".
  assert.equal(capabilityWords({}), null)
  assert.equal(capabilityWords(null), null)
})

test('the manager row carries the capability words, or null when the daemon did not say', () => {
  const [row] = harnessRows(
    cap([
      {
        id: 'grok',
        available: true,
        honors: { effort: true, fast: false, mcp: false, plugins: false, tools: false, attachments: false },
      },
    ]),
    [],
  )
  assert.equal(row.caps.length, 6)
  const [unknown] = harnessRows(cap([{ id: 'grok', available: true }]), [])
  assert.equal(unknown.caps, null)
})

test('an available signed-in harness is Ready with the daemon probe words', () => {
  const [row] = harnessRows(
    cap([{ id: 'claude', available: true, signed_in: true, version: '2.1.287', path: '/usr/local/bin/claude' }]),
    [],
  )
  assert.equal(row.statusLabel, 'Ready')
  assert.ok(row.ready)
  assert.equal(row.action, 'none')
  assert.equal(row.detail, '2.1.287 · /usr/local/bin/claude')
})

test('a signed-out harness offers sign-in only where the daemon can relay it', () => {
  const rows = harnessRows(
    cap([
      { id: 'codex', available: true, signed_in: false, path: '/usr/bin/codex' },
      { id: 'opencode', available: true, signed_in: false, path: '/usr/bin/opencode' },
    ]),
    [],
  )
  assert.equal(rows[0].statusLabel, 'Not signed in')
  assert.equal(rows[0].action, 'signin')
  assert.equal(rows[1].statusLabel, 'Not signed in')
  assert.equal(rows[1].action, 'none')
})

test('every missing harness with a spec installs - grok and agy included, no command shown', () => {
  const ids = ['claude', 'codex', 'grok', 'pi', 'opencode', 'agy']
  const rows = harnessRows(
    cap(ids.map((id) => ({ id, available: false }))),
    ids.map((id) => spec(id)),
  )
  for (const row of rows) {
    assert.equal(row.statusLabel, 'Not installed')
    assert.equal(row.action, 'install')
    assert.equal(row.detail, null)
  }
})

test('a spec that needs a runtime waits for the go-ahead', () => {
  const [pi, opencode] = harnessRows(
    cap([
      { id: 'pi', available: false },
      { id: 'opencode', available: false },
    ]),
    [spec('pi', { needs_runtime: true }), spec('opencode', { source: 'release', note: 'Unpacks its release.' })],
  )
  assert.equal(pi.action, 'install-runtime')
  assert.equal(opencode.action, 'install')
  assert.equal(opencode.detail, null)
})

test('a missing harness with no spec says so, once the machine has answered', () => {
  const [row] = harnessRows(cap([{ id: 'claude', available: false }]), [])
  assert.equal(row.action, 'none')
  assert.equal(row.detail, 'No installer for this platform.')
  assert.equal(row.detailKind, 'note')
})

test('while the machines list has not answered, a missing row stays quiet', () => {
  const [row] = harnessRows(cap([{ id: 'grok', available: false }]), null)
  assert.equal(row.action, 'none')
  assert.equal(row.detail, null)
})

test('a plugin harness is named by the daemon and rows like a built-in one', () => {
  const [row] = harnessRows(
    cap([
      {
        id: 'freebuff',
        name: 'Freebuff',
        plugin: 'freebuff@local',
        available: true,
        signed_in: true,
        version: '0.2.35',
      },
    ]),
    [],
  )
  assert.equal(row.name, 'Freebuff')
  assert.equal(row.statusLabel, 'Ready')
  assert.equal(row.detail, '0.2.35')
})

test('the daemon\'s reason a harness is unusable is the row\'s detail', () => {
  // A harness the daemon reports with a problem is not usable, and the
  // sentence it sent says why: "not installed" alone would leave the person
  // guessing at a missing CLI or an API this build does not speak.
  const [row] = harnessRows(
    cap([{ id: 'freebuff', name: 'Freebuff', available: false, problem: 'Freebuff CLI not found' }]),
    [],
  )
  assert.equal(row.name, 'Freebuff')
  assert.equal(row.statusLabel, 'Not installed')
  assert.equal(row.detail, 'Freebuff CLI not found')
  assert.equal(row.detailKind, 'note')
  assert.equal(row.action, 'none')
})

test('a problem does not take away a missing harness\'s install button', () => {
  const [row] = harnessRows(
    cap([{ id: 'claude', available: false, problem: 'The claude CLI is not on this computer.' }]),
    [spec('claude')],
  )
  assert.equal(row.action, 'install')
  assert.equal(row.detail, 'The claude CLI is not on this computer.')
  assert.equal(row.detailKind, 'note')
})

test('an empty problem is no problem', () => {
  const [row] = harnessRows(cap([{ id: 'claude', available: false, problem: '   ' }]), [])
  assert.equal(row.detail, 'No installer for this platform.')
})

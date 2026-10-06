import assert from 'node:assert/strict'
import test from 'node:test'

const { harnessRows, MANUAL_NOTE } = await import('./harness-manager.js')

const cap = (harnesses) => ({ harnesses })
const spec = (harnessId) => ({ harness_id: harnessId, name: harnessId, command: `npm install -g pkg-${harnessId}` })

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

test('an available harness that says it is signed out carries its own words', () => {
  const [row] = harnessRows(cap([{ id: 'codex', available: true, signed_in: false, path: '/usr/bin/codex' }]), [])
  assert.equal(row.statusLabel, 'Not signed in')
  assert.ok(!row.ready)
  assert.equal(row.action, 'none')
})

test('a missing harness with an install spec gets the verbatim command and an install action', () => {
  const [row] = harnessRows(cap([{ id: 'pi', available: false }]), [spec('pi')])
  assert.equal(row.statusLabel, 'Missing')
  assert.equal(row.action, 'install')
  assert.equal(row.detail, 'npm install -g pkg-pi')
})

test('grok and agy never get an install row - their own installers own them', () => {
  const rows = harnessRows(
    cap([
      { id: 'grok', available: false },
      { id: 'agy', available: false },
    ]),
    [],
  )
  for (const row of rows) {
    assert.equal(row.action, 'manual')
    assert.equal(row.detail, MANUAL_NOTE[row.id])
    assert.ok(!row.detail.includes('npm install'))
  }
})

test('a missing harness with no spec and no note says not found, once the machine has answered', () => {
  const [row] = harnessRows(cap([{ id: 'unknown', available: false }]), [])
  assert.equal(row.action, 'manual')
  assert.equal(row.detail, 'Not found on this machine.')
})

test('while the machines list has not answered, a specless missing row stays quiet', () => {
  const [row] = harnessRows(cap([{ id: 'unknown', available: false }]), null)
  assert.equal(row.action, 'none')
  assert.equal(row.detail, null)
  // The known manual harnesses still say their words - that truth is
  // not the machine's to report.
  const [grok] = harnessRows(cap([{ id: 'grok', available: false }]), null)
  assert.equal(grok.action, 'manual')
})

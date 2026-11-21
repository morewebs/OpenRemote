import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(file, import.meta.url), 'utf8')

test('machine details opens the shared Apps manager on that machine', () => {
  const machine = source('./MachineModal.jsx')
  assert.match(machine, /<HarnessManagerModal\s+initialDeviceId=\{isMe \? null : device\.id\}/)
  assert.doesNotMatch(machine, /installHarness\(/)
})

test('the installer command stays behind a disclosure in Apps', () => {
  const apps = source('./HarnessManagerModal.jsx')
  assert.match(apps, /<details className="dv-install-more">[\s\S]*?Show the command[\s\S]*?\{row\.installCommand\}[\s\S]*?<\/details>/)
  assert.match(apps, /await installHarness\(machineId, harnessId, \{ \.\.\.options, deviceId \}\)/)
})

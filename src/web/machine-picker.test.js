// The browser is a device of the account, not a machine. Its New chat picker
// must list the account's machines and never "This computer".

import assert from 'node:assert/strict'
import test from 'node:test'
import { machinePickerItems } from '../cloud.js'

const devices = [
  { id: 'web-1', name: 'OpenRemote Web (Chrome)', kind: 'web', online: true },
  { id: 'desk-1', name: 'laptop', kind: 'desktop', online: true },
  { id: 'm1', name: 'build-box', kind: 'machine', online: true },
  { id: 'm2', name: 'studio-mini', kind: 'machine', online: false },
]

test('a web session offers machines only, online first', () => {
  const rows = machinePickerItems(devices, 'web-1', { phone: true })
  assert.deepEqual(rows.map((row) => row.id), ['m1', 'm2'])
  assert.ok(!rows.some((row) => row.id === null), 'no "This computer" row')
  assert.ok(!rows.some((row) => row.id === 'web-1'), 'the browser is not a target')
  assert.ok(!rows.some((row) => row.id === 'desk-1'), 'a desktop that is not a machine is not a target')
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { chatAccess, machinePickerItems, sessionsForMode, signedIn } from './cloud.js'

const me = 'dev-me'
const devices = [
  { id: me, name: 'laptop', kind: 'desktop', online: true },
  { id: 'dev-box', name: 'build-box', kind: 'machine', online: true, platform: 'linux' },
  { id: 'dev-mini', name: 'mac-mini', kind: 'machine', online: false, platform: 'macos' },
  { id: 'dev-other', name: 'work-laptop', kind: 'desktop', online: true },
]
const online = { state: 'online' }

test('Local lists what runs here; Cloud lists every synced chat', () => {
  const sessions = [
    { id: 'private' },
    { id: 'synced-here', executor: me },
    { id: 'remote', executor: 'dev-box' },
  ]
  assert.deepEqual(sessionsForMode(sessions, 'local', me).map((s) => s.id), ['private', 'synced-here'])
  assert.deepEqual(sessionsForMode(sessions, 'cloud', me).map((s) => s.id), ['synced-here', 'remote'])
  assert.deepEqual(sessionsForMode(undefined, 'cloud', me), [])
})

test('a chat on another device is reachable only through a machine that is online', () => {
  assert.equal(chatAccess({ id: 'a' }, devices, me, online), 'here')
  assert.equal(chatAccess({ executor: me }, devices, me, online), 'here')
  assert.equal(chatAccess({ executor: 'dev-box' }, devices, me, online), 'remote')
  assert.equal(chatAccess({ executor: 'dev-mini' }, devices, me, online), 'offline')
  assert.equal(chatAccess({ executor: 'dev-other' }, devices, me, online), 'not-machine')
  assert.equal(chatAccess({ executor: 'dev-gone' }, devices, me, online), 'gone')
  assert.equal(chatAccess({ executor: 'dev-box' }, devices, me, { state: 'signed_out' }), 'signed-out')
  assert.equal(signedIn({ state: 'offline' }), true, 'offline is still signed in')
})

test('new Cloud chats can run here or on a machine, online machines first', () => {
  const items = machinePickerItems(devices, me)
  assert.deepEqual(items.map((i) => i.name), ['This computer', 'build-box', 'mac-mini'])
  assert.equal(items[0].id, null)
  assert.equal(items[2].online, false)
  assert.deepEqual(machinePickerItems([], me).map((i) => i.name), ['This computer'])
})

test('new devices are flagged once, after a baseline', async () => {
  const { newDevices, madeMachineElsewhere } = await import('./cloud.js')
  assert.deepEqual(newDevices(devices, null, me), [], 'no baseline yet: nothing flagged')
  const known = devices.map((d) => d.id)
  assert.deepEqual(newDevices(devices, known, me), [])
  const joined = [...devices, { id: 'dev-new', name: 'stranger', kind: 'desktop', online: true }]
  assert.deepEqual(newDevices(joined, known, me).map((d) => d.name), ['stranger'])
  assert.equal(madeMachineElsewhere('machine', 'desktop', null), true)
  assert.equal(madeMachineElsewhere('machine', 'desktop', 'machine'), false, 'made a machine here')
  assert.equal(madeMachineElsewhere('machine', 'machine', null), false)
  assert.equal(madeMachineElsewhere('desktop', 'desktop', null), false)
})

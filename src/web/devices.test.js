// The device list, in the words the machine picker shows: a machine is a
// machine, a phone is not offered as one, and a machine that is asleep is
// shown asleep rather than hidden or renamed.

import assert from 'node:assert/strict'
import test from 'node:test'
import { machineLine, machineOf, machinesOf, pickRefusal } from './devices.js'

const payload = {
  account: { email: 'owner@example.test' },
  devices: [
    { id: 'm1', name: 'build-box', kind: 'machine', online: true, platform: 'linux', app_version: '0.4.8' },
    { id: 'm2', name: 'studio-mini', kind: 'machine', online: false, platform: 'macos' },
    { id: 'd1', name: 'laptop', kind: 'desktop', online: true },
    { id: 'p1', name: 'phone', kind: 'android', online: true },
    { id: 'w1', name: 'this browser', kind: 'web', online: true },
  ],
}

test('only a machine is offered as a machine', () => {
  const machines = machinesOf(payload)
  assert.deepEqual(machines.map((m) => m.id), ['m1', 'm2'])
  // A desktop, a phone and this browser are devices of the account, not
  // computers that run chats: offering one would be an offer the link cannot
  // keep, since only a machine answers the machine tier.
  assert.equal(machineOf({ id: 'p1', name: 'phone', kind: 'phone', online: true }), null)
  assert.equal(machineOf({ id: 'd1', name: 'laptop', kind: 'desktop' }), null)
  assert.equal(machineOf({ id: 'w1', name: 'web', kind: 'web' }), null)
})

test('an offline machine is shown offline, and offline ones sit below the online ones', () => {
  const machines = machinesOf(payload)
  assert.equal(machines[0].id, 'm1', 'the online machine leads')
  assert.equal(machineLine(machines[0]), 'Online')
  assert.equal(machineLine(machines[1]), 'Offline')
  // Ties keep name order, so the list does not reshuffle between polls.
  const tied = machinesOf({
    devices: [
      { id: 'b', name: 'beta', kind: 'machine', online: true },
      { id: 'a', name: 'alpha', kind: 'machine', online: true },
    ],
  })
  assert.deepEqual(tied.map((m) => m.name), ['alpha', 'beta'])
})

test('a machine with no name still reads as a machine', () => {
  const [m] = machinesOf({ devices: [{ id: 'm3', kind: 'machine', online: true, name: '   ' }] })
  assert.equal(m.name, 'An unnamed machine')
  assert.equal(machineLine(m), 'Online')
})

test('a machine that is offline says why it cannot be picked; an online one has no refusal', () => {
  const [online, offline] = machinesOf(payload)
  assert.equal(pickRefusal(online), null)
  assert.match(pickRefusal(offline), /offline/i)
  assert.match(pickRefusal(null), /not a machine/i)
})

test('a list that is missing, empty or malformed is an empty list', () => {
  assert.deepEqual(machinesOf(null), [])
  assert.deepEqual(machinesOf({}), [])
  assert.deepEqual(machinesOf({ devices: 'nope' }), [])
  assert.deepEqual(machinesOf({ devices: [null, 7, { id: 'x', kind: 'machine' }] }).map((m) => m.id), ['x'], 'the one usable row survives')
})

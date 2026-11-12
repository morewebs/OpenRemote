import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { chatAccess, cloudAnswered, kindLabel, machinePickerItems, sessionsForMode, signedIn } from './cloud.js'

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

test('an unanswered device list is unknown, not evidence that a device left', () => {
  // The list is [] before the first read lands, and the daemon answers [] while
  // its relay link is still coming up: both are silence, not an answer.
  assert.equal(chatAccess({ executor: 'dev-box' }, [], me, { state: 'connecting' }), 'unknown')
  assert.equal(chatAccess({ executor: 'dev-box' }, [], me, { state: 'online' }), 'unknown')
  assert.equal(
    chatAccess({ executor: 'dev-box' }, null, me, online),
    'unknown',
    'a list that has not arrived is the same as an empty one',
  )
  // A daemon that answers always includes itself, so an answered list is never
  // empty; a populated list missing the device is the one thing that means gone.
  assert.equal(chatAccess({ executor: 'dev-gone' }, devices, me, online), 'gone')
  // A list that is not an array is daemon damage, and reads as no answer either.
  assert.equal(chatAccess({ executor: 'dev-box' }, { devices: [] }, me, online), 'unknown')
})

test('new Cloud chats can run here or on a machine, online machines first', () => {
  const items = machinePickerItems(devices, me)
  assert.deepEqual(items.map((i) => i.name), ['This computer', 'build-box', 'mac-mini'])
  assert.equal(items[0].id, null)
  assert.equal(items[2].online, false)
  assert.deepEqual(machinePickerItems([], me).map((i) => i.name), ['This computer'])
})

test('on a phone the picker is the machines alone', async () => {
  const { isPhone, platformLabel } = await import('./cloud.js')
  const items = machinePickerItems(devices, me, { phone: true })
  assert.deepEqual(items.map((i) => i.name), ['build-box', 'mac-mini'])
  assert.deepEqual(machinePickerItems([], me, { phone: true }), [])
  assert.equal(isPhone({ platform: 'android' }), true)
  assert.equal(isPhone({ platform: 'linux' }), false)
  assert.equal(isPhone(null), false)
  assert.equal(platformLabel('android'), 'Android')
  assert.equal(platformLabel('haiku'), 'haiku')
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

test('a sign-in reason is the daemon sentence, capitalised, or nothing at all', async () => {
  const { daemonReason } = await import('./cloud.js')
  assert.equal(
    daemonReason({ state: 'relink', error: 'this computer is no longer in your account' }),
    'This computer is no longer in your account',
    'the daemon sentence is shown as written, in sentence case',
  )
  assert.equal(daemonReason({ state: 'relink', error: '' }), null, 'an empty sentence is no reason')
  assert.equal(daemonReason({ state: 'relink' }), null, 'no sentence at all is no reason')
  assert.equal(daemonReason(undefined), null, 'no Cloud state is no reason')
})

test('the alert under the door never repeats the body', async () => {
  const { cloudAlert, daemonReason } = await import('./cloud.js')
  const sent = 'this computer is no longer in your account'
  const relink = { state: 'relink', error: sent }
  const body = daemonReason(relink)

  // The shape the bug was: a failed sign-in click answers with the daemon's
  // own sentence, which the body is already showing, in sentence case.
  assert.equal(cloudAlert({ clickError: sent, cloud: relink, body }), null)
  assert.equal(cloudAlert({ clickError: body, cloud: relink, body }), null)

  // A click that failed for its own reason is news, and stays.
  const refused = 'the sign-in page did not answer'
  assert.equal(cloudAlert({ clickError: refused, cloud: relink, body }), refused)

  // Outside relink the daemon's sentence has no body of its own, so it is
  // the alert.
  const revoked = { state: 'revoked', error: 'it was removed from your account' }
  assert.equal(cloudAlert({ cloud: revoked, body: 'This computer was removed' }), daemonReason(revoked))

  // While the browser is open the door is not asking for anything: no alert.
  assert.equal(cloudAlert({ clickError: refused, cloud: { state: 'signing_in' }, signingIn: true }), null)

  // Nothing to say at all.
  assert.equal(cloudAlert({ cloud: { state: 'online' }, body: 'Connected to Cloud.' }), null)
  assert.equal(cloudAlert({}), null)
})

test('a device row with the wrong shape cannot throw the machine picker', () => {
  // A row with no name used to take `localeCompare` down with a TypeError,
  // which the boundary showed as the whole New chat view failing.
  const malformed = [
    { id: 'dev-box', kind: 'machine', online: true },
    { id: 'dev-mini', kind: 'machine', online: false, name: 7 },
    { id: me, kind: 'machine', online: true, name: 'laptop' },
  ]
  const items = machinePickerItems(malformed, me)
  assert.deepEqual(items.map((i) => i.id), [null, 'dev-box', 'dev-mini'])
  assert.equal(items[1].name, '', 'a missing name is no name, not a throw')
  assert.equal(items[2].name, '7', 'a number keeps its text')
  // The list itself can arrive as the wrong shape too.
  assert.deepEqual(machinePickerItems({}, me).map((i) => i.name), ['This computer'])
  assert.deepEqual(machinePickerItems(null, me).map((i) => i.name), ['This computer'])
})

test('an unanswered Cloud view is not a signed-out one', () => {
  // The poll gap at every start and every daemon restart: nothing has been
  // asked yet, so nobody may be told they are signed out.
  assert.equal(cloudAnswered(null), false)
  assert.equal(cloudAnswered(undefined), false)
  assert.equal(cloudAnswered({}), false)
  assert.equal(cloudAnswered({ account: { email: 'a@b.c' } }), false, 'an email without a state is not an answer')
  // Once the daemon says a state, whatever it is, the view has been answered.
  for (const state of ['online', 'offline', 'connecting', 'signing_in', 'relink', 'revoked', 'signed_out']) {
    assert.equal(cloudAnswered({ state }), true, state)
  }
})

test('the two surfaces that show the sign-in state use the one rule for it', () => {
  // Wiring pin, source text: the rule above is only worth having if both
  // surfaces ask it. A regression that goes back to `cloud?.state` in one of
  // them would leave this suite green otherwise, because the rule's own tests
  // would still pass.
  const row = readFileSync(new URL('./CloudStateRow.jsx', import.meta.url), 'utf8')
  const account = readFileSync(new URL('./AccountView.jsx', import.meta.url), 'utf8')
  assert.match(row, /cloudAnswered\(cloud\)/, 'the account row asks whether Cloud has answered')
  assert.match(row, /Checking the Cloud sign-in/, 'and says it is checking, not signed out')
  assert.match(account, /cloudAnswered\(cloud\)/, 'the account view asks the same question')
  assert.match(account, /'Checking'/, 'and shows the same waiting word')
})

test('signing out reports its own failure instead of claiming success', () => {
  // Wiring pin, source text: the sign-out handler is inline, so the properties
  // that matter are that it catches, that it sets the row's error, and that it
  // says "Signing out" while it runs. A rejected promise used to leave the row
  // reading "Signed in ... reachable".
  const row = readFileSync(new URL('./CloudStateRow.jsx', import.meta.url), 'utf8')
  const handler = row.split('const signOut = async () => {')[1]?.split('const cancel')[0] ?? ''
  assert.ok(handler.includes('await signOutCloud()'), 'the handler awaits the sign-out')
  assert.match(handler, /catch \(err\)/, 'and catches a rejection')
  assert.match(handler, /setError\(errorCopy\(err\)\)/, 'and shows it the way a failed sign-in is shown')
  assert.match(row, /'Signing out'/, 'the row says what it is doing while it runs')
})

test('a device names its kind: a browser is a web console, never a desktop', () => {
  assert.equal(kindLabel({ kind: 'web', platform: 'web' }), 'Web console')
  assert.equal(kindLabel({ kind: 'machine', platform: 'linux' }), 'Machine')
  assert.equal(kindLabel({ kind: 'desktop', platform: 'windows' }), 'Desktop')
  assert.equal(kindLabel({ kind: 'desktop', platform: 'android' }), 'Phone')
})

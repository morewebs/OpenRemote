import assert from 'node:assert/strict'
import test from 'node:test'
import { mobileHomeState } from './mobile-home.js'

const machine = { id: 'workstation', name: 'Workstation', online: true }

test('a phone without a computer offers the Machines screen', () => {
  const state = mobileHomeState({ machine: null, apps: null })
  assert.equal(state.kind, 'no-machine')
  assert.equal(state.action, 'machines')
  assert.equal(state.title, 'Connect a computer')
})

test('offline takes precedence over an empty or previously loaded App list', () => {
  for (const apps of [null, [], [{ id: 'codex' }]]) {
    const state = mobileHomeState({ machine: { ...machine, online: false }, apps })
    assert.equal(state.kind, 'offline')
    assert.equal(state.action, 'machine')
    assert.equal(state.title, 'Your computer is offline')
    assert.match(state.detail, /OpenRemote/)
  }
})

test('unanswered Apps are still connecting, not missing', () => {
  const state = mobileHomeState({ machine, apps: null })
  assert.equal(state.kind, 'loading')
  assert.equal(state.action, null)
  assert.equal(state.title, 'Connecting to your computer')
})

test('an answered empty list directs App setup to the computer', () => {
  const state = mobileHomeState({ machine, apps: [] })
  assert.equal(state.kind, 'no-apps')
  assert.equal(state.action, 'machines')
  assert.match(state.detail, /on your computer/)
})

test('an online computer with an App needs no setup warning', () => {
  assert.equal(mobileHomeState({ machine, apps: [{ id: 'codex', available: true }] }), null)
})

test('an unanswered computer inventory does not ask an existing user to connect again', () => {
  const state = mobileHomeState({ machine: null, apps: null, devicesAnswered: false })
  assert.equal(state.kind, 'loading-machines')
  assert.equal(state.title, 'Finding your computers')
  assert.equal(state.action, null)
})

test('installed Apps that are all turned off need enabling, not reinstalling', () => {
  const state = mobileHomeState({ machine, apps: [{ id: 'codex', available: true, enabled: false }] })
  assert.equal(state.kind, 'disabled-apps')
  assert.match(state.detail, /Plugins on your computer/)
  assert.doesNotMatch(state.detail, /Install/)
})

test('an inventory containing only uninstalled Apps directs installation', () => {
  assert.equal(mobileHomeState({ machine, apps: [{ id: 'codex', available: false }] }).kind, 'no-apps')
})

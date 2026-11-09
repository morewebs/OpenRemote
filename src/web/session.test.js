// The web console's pending sign-in storage. The browser is the device, so
// this is the only place a PKCE verifier and state can wait for the gateway's
// return. Two sign-ins must not overwrite each other: a second tab or a
// double click used to leave the first callback with somebody else's state.

import assert from 'node:assert/strict'
import test from 'node:test'

const store = new Map()
globalThis.localStorage = {
  getItem: (key) => (store.has(key) ? store.get(key) : null),
  setItem: (key, value) => store.set(key, String(value)),
  removeItem: (key) => store.delete(key),
}

const { clearPending, loadPending, savePending } = await import('./session.js')

const pending = (state, verifier = `v-${state}`) => ({
  state,
  verifier,
  redirect: 'https://openremote.moreweb.space/app/',
  api: 'https://api.moreweb.space/v1/openremote',
  issuer: 'https://auth.moreweb.space/v1/auth',
})

test('two sign-ins keep their own state and verifier', () => {
  store.clear()
  assert.equal(savePending(pending('a')), true)
  assert.equal(savePending(pending('b')), true)
  assert.equal(loadPending('a').verifier, 'v-a')
  assert.equal(loadPending('b').verifier, 'v-b')
})

test('clearing one sign-in leaves the other for its own callback', () => {
  store.clear()
  savePending(pending('a'))
  savePending(pending('b'))
  clearPending('a')
  assert.equal(loadPending('a'), null)
  assert.equal(loadPending('b').verifier, 'v-b')
  clearPending('b')
  assert.equal(loadPending('b'), null)
})

test('a pending written by the old single-slot build still finishes', () => {
  store.clear()
  store.set('openremote-web-signin', JSON.stringify(pending('legacy')))
  assert.equal(loadPending('legacy').verifier, 'v-legacy')
})

test('a sign-in that can no longer be in flight is not returned', () => {
  store.clear()
  store.set(
    'openremote-web-signin',
    JSON.stringify({ stale: { ...pending('stale'), savedAt: Date.now() - 60 * 60_000 } }),
  )
  assert.equal(loadPending('stale'), null)
})

test('storage that refuses the write says so instead of pretending', () => {
  store.clear()
  const setItem = globalThis.localStorage.setItem
  globalThis.localStorage.setItem = () => {
    throw new Error('denied')
  }
  assert.equal(savePending(pending('a')), false)
  globalThis.localStorage.setItem = setItem
})

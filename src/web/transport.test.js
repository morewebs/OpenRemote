// The transport is what makes the console's own fetch the link's fetch.
// Pinned here because the console module itself is JSX and cannot be imported
// by the suite: this checks the functions it calls, and the DaemonApi built
// from them never touches the global fetch.

import assert from 'node:assert/strict'
import test from 'node:test'
import { DaemonApi } from '../api/daemon.js'
import { PEER_ORIGIN, setWebTransport, webFetch, webTransport } from './transport.js'

test('there is no transport until a machine is picked', () => {
  assert.equal(webTransport(), null)
  assert.equal(webFetch(), null)
  // The origin is a placeholder that nothing resolves: a request's path is
  // what crosses the link, so the host in the URL cannot be dialled by anyone.
  assert.match(PEER_ORIGIN, /^https:\/\/\S+\.invalid$/)
})

test('an installed transport answers the client instead of the global fetch', async () => {
  const real = globalThis.fetch
  let globalCalls = 0
  globalThis.fetch = () => {
    globalCalls += 1
    throw new Error('the global fetch must not be reached')
  }
  try {
    const seen = []
    const fetchShaped = async (url) => {
      seen.push(url)
      return new Response(JSON.stringify({ ok: true }), { status: 200 })
    }
    setWebTransport({ device: { id: 'm1', name: 'build-box' }, fetch: fetchShaped })
    assert.equal(webFetch(), fetchShaped)
    const api = new DaemonApi(`${PEER_ORIGIN}/`, 'web-link', '', webFetch())
    assert.deepEqual(await api.health(), { ok: true })
    assert.deepEqual(seen, [`${PEER_ORIGIN}/healthz`], 'the request crossed the link, not the network')
    assert.equal(globalCalls, 0)
  } finally {
    setWebTransport(null)
    globalThis.fetch = real
  }
})

test('a transport without a fetch is not installed', () => {
  setWebTransport({ device: { id: 'm1' } })
  assert.equal(webTransport(), null)
  assert.equal(webFetch(), null)
})

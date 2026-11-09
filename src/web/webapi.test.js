// The web client's one routing rule: a machine id is not a `/devices/{id}`
// prefix. The desktop daemon forwards a peer request under that prefix because
// it is asking another daemon; the browser's peer fetch already carries the
// path to the machine, so the client must start at the root.

import assert from 'node:assert/strict'
import test from 'node:test'
import { webApi } from './webapi.js'
import { PEER_ORIGIN } from './transport.js'

function fakePeer() {
  const calls = []
  return {
    calls,
    fetchFor(machineId) {
      return async (url, init) => {
        calls.push({ machineId, url, method: init?.method })
        return new Response(JSON.stringify({ ok: true }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        })
      }
    },
  }
}

test('a web client calls the machine at the peer root, never under /devices', async () => {
  const peer = fakePeer()
  const api = webApi(peer, 'machine-1')
  assert.equal(api.url, PEER_ORIGIN)
  assert.equal(api.prefix, '')
  await api.call('GET', '/sync/summary')
  assert.equal(peer.calls.length, 1)
  assert.equal(peer.calls[0].machineId, 'machine-1')
  assert.equal(peer.calls[0].url, `${PEER_ORIGIN}/sync/summary`)
  assert.ok(!peer.calls[0].url.includes('/devices/'))
})

test('a client without a machine refuses instead of asking some default', () => {
  const peer = fakePeer()
  assert.throws(() => webApi(peer, null), /machine/)
})

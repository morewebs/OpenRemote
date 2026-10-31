// Two browser peers, linked to each other, with no network and no gateway.
//
// The module is the Rust link compiled to WebAssembly:
//
//   cd daemon
//   cargo build --release --target wasm32-unknown-unknown -p openremote-webpeer
//   copy target/wasm32-unknown-unknown/release/openremote_webpeer.wasm ../src/peer/openremote-webpeer.wasm
//
// (On Windows the second line is `Copy-Item` from the same path under
// source/wasm32-unknown-unknown/release.) The test fails with this instruction
// when the file is missing rather than skipping: a green run that never loaded
// the module would prove nothing about the protocol.

import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'

import { HEAD, RDATA, REND, RESET, Rpc, frame, parse } from './rpc.js'
import { WebPeer, hex, idBytes } from './relay.js'
import { base64url } from './identity.js'
import { loadWasm } from './wasm.js'

const WASM = fileURLToPath(new URL('./openremote-webpeer.wasm', import.meta.url))
const BUILD = [
  '  cd daemon',
  '  cargo build --release --target wasm32-unknown-unknown -p openremote-webpeer',
  '  copy target/wasm32-unknown-unknown/release/openremote_webpeer.wasm ../src/peer/openremote-webpeer.wasm',
].join('\n')

async function moduleBytes() {
  try {
    return await readFile(WASM)
  } catch {
    throw new Error(`the web peer module is missing (${WASM}). Build it with:\n${BUILD}`)
  }
}

const ACCOUNT = 'acct-1'
const ID_A = new Uint8Array(16).fill(1)
const ID_B = new Uint8Array(16).fill(2)
const encoder = new TextEncoder()

function key(seed) {
  return new Uint8Array(32).fill(seed)
}

/** Two linked handles, the way the relay hands the handshake frames over. */
async function linkedPair() {
  const api = await loadWasm(await moduleBytes())
  const a = api.newPeer(key(1))
  const b = api.newPeer(key(2))
  assert.ok(a && b, 'both keys mint a handle')
  const pubA = api.pub(a)
  const pubB = api.pub(b)

  const hs1 = api.initiate(a, pubB, ACCOUNT, ID_A, ID_B)
  assert.equal(hs1[0], 1, 'the initiator sends a handshake frame')
  const hs2 = api.respond(b, pubA, ACCOUNT, ID_B, ID_A, hs1)
  assert.equal(hs2[0], 2, 'the responder answers with its own')
  assert.equal(api.complete(a, hs2), true, 'the initiator is up')
  return { api, a, b, pubA, pubB }
}

test('two handles link, and a sealed request crosses between them', async () => {
  const { api, a, b } = await linkedPair()
  const request = encoder.encode(JSON.stringify({ method: 'GET', path: '/sessions' }))
  const sealed = api.seal(a, request)
  assert.equal(sealed[0], 3, 'a transport frame')
  assert.ok(!Buffer.from(sealed).includes('sessions'), 'the relay would only see ciphertext')
  assert.deepEqual(api.open(b, sealed), request)
  // The same frame again is not a message.
  assert.throws(() => api.open(b, sealed), /frame/)
})

test('a tampered frame is refused, not read as an empty message', async () => {
  const { api, a, b } = await linkedPair()
  const sealed = api.seal(a, encoder.encode('hello'))
  const tampered = sealed.slice()
  tampered[tampered.length - 1] ^= 0xff
  assert.throws(() => api.open(b, tampered), /frame/)
  // A frame of the wrong kind is refused too, and so is a truncated one.
  assert.throws(() => api.open(b, new Uint8Array([1, 9, 9, 9])), /frame/)
  assert.throws(() => api.open(b, new Uint8Array([3])), /frame/)
})

test('a wrong peer key or another account never links', async () => {
  const api = await loadWasm(await moduleBytes())
  const a = api.newPeer(key(1))
  const b = api.newPeer(key(2))
  const stranger = api.newPeer(key(3))
  const pubA = api.pub(a)
  const hs1 = api.initiate(a, api.pub(stranger), ACCOUNT, ID_A, ID_B)
  assert.throws(() => api.respond(b, pubA, ACCOUNT, ID_B, ID_A, hs1), /handshake/)
  const other = api.initiate(a, api.pub(b), 'acct-2', ID_A, ID_B)
  assert.throws(() => api.respond(b, pubA, ACCOUNT, ID_B, ID_A, other), /handshake/)
})

test('a blank private key mints no device', async () => {
  const api = await loadWasm(await moduleBytes())
  assert.equal(api.newPeer(new Uint8Array(32)), 0)
  assert.equal(api.error(), 1, 'the reason is the key, not a state')
})

/**
 * Two Rpc instances through two linked handles, with one acting as the daemon
 * side of a request: it answers with HEAD, RDATA chunks and REND, and answers
 * RESET for a path that asks for it.
 */
async function rpcPair(responder) {
  const { api, a, b } = await linkedPair()
  const left = new Rpc({ send: (bytes) => right.onMessage(api.open(b, api.seal(a, bytes))) })
  const right = new Rpc({ send: (bytes) => left.onMessage(api.open(a, api.seal(b, bytes))) })
  right.onMessage = (bytes) => {
    const parsed = parse(bytes)
    if (!parsed) return
    const [kind, stream, body] = parsed
    if (kind !== 0x10) return
    const header = JSON.parse(new TextDecoder().decode(body))
    responder({ stream, header, send: (out) => right.send(out) })
  }
  return { left, right }
}

test('the rpc framing reassembles a body larger than one chunk', async () => {
  const big = JSON.stringify({
    rows: Array.from({ length: 900 }, (_, at) => ({ at, pad: 'x'.repeat(80) })),
  })
  assert.ok(big.length > 48 * 1024, 'the body really is larger than one chunk')
  const { left } = await rpcPair(({ stream, header, send }) => {
    const body = encoder.encode(big)
    send(frame(HEAD, stream, encoder.encode(JSON.stringify({ status: 200, content_type: 'application/json' }))))
    for (let at = 0; at < body.length; at += 40 * 1024) {
      send(frame(RDATA, stream, body.subarray(at, at + 40 * 1024)))
    }
    send(frame(REND, stream, new Uint8Array(0)))
    assert.equal(header.method, 'GET')
  })
  const answer = await left.call('GET', '/sessions', null, { timeoutMs: 2_000 })
  assert.equal(answer.status, 200)
  assert.equal(answer.contentType, 'application/json')
  assert.equal(new TextDecoder().decode(answer.body), big)
})

test('a RESET settles a pending call with an error instead of hanging', async () => {
  const { left } = await rpcPair(({ stream, send }) => {
    send(frame(RESET, stream, encoder.encode(JSON.stringify({ error: 'the peer said no' }))))
  })
  await assert.rejects(left.call('GET', '/sessions', null, { timeoutMs: 2_000 }), /the peer said no/)
  assert.equal(left.streams.size, 0, 'nothing is left waiting')
})

test('the caller abort settles the call and sends a RESET', async () => {
  const seen = []
  let abortedStream = null
  const { left } = await rpcPair(({ stream }) => {
    abortedStream = stream
  })
  left.send = (bytes) => seen.push(bytes)
  const controller = new AbortController()
  const call = left.call('GET', '/slow', null, { signal: controller.signal, timeoutMs: 2_000 })
  controller.abort()
  await assert.rejects(call, /aborted/)
  assert.equal(abortedStream, null, 'the responder never answered')
  const reset = seen.map((one) => parse(one)).find((one) => one && one[0] === RESET)
  assert.ok(reset, 'a RESET went out')
})

/** A device id as the text the registry uses, from its 16 bytes. */
function uuidOf(bytes) {
  const text = hex(bytes)
  return `${text.slice(0, 8)}-${text.slice(8, 12)}-${text.slice(12, 16)}-${text.slice(16, 20)}-${text.slice(20)}`
}

/**
 * Two sockets that behave as the relay does: a frame goes out as
 * `[destination][payload]` and arrives as `[sender][payload]`, so neither side
 * ever reads a destination it sent itself.
 */
function relayPair(frames, uintA, uintB) {
  let a = null
  let b = null
  const endpoint = (self, peerOf) => ({
    readyState: 1,
    onmessage: null,
    onclose: null,
    onerror: null,
    send(bytes) {
      frames.push(Array.from(bytes.subarray(16)))
      const out = new Uint8Array(bytes.length)
      out.set(idBytes(self), 0)
      out.set(bytes.subarray(16), 16)
      const far = peerOf()
      if (far?.onmessage) far.onmessage({ data: out })
    },
    close() {
      this.readyState = 3
    },
  })
  a = endpoint(uintA, () => b)
  b = endpoint(uintB, () => a)
  return {
    a: () => a,
    b: () => b,
  }
}

/** Two peers wired to each other, with a machine behind B that answers one
 *  path and nothing else. */
async function socketPair({ swap = false } = {}) {
  const api = await loadWasm(await moduleBytes())
  const me = api.newPeer(key(1))
  const machine = api.newPeer(key(2))
  const frames = []
  const idA = swap ? uuidOf(ID_B) : uuidOf(ID_A)
  const idB = swap ? uuidOf(ID_A) : uuidOf(ID_B)
  const transport = relayPair(frames, idA, idB)
  const events = []
  const client = new WebPeer({
    api: 'http://relay.test',
    wasm: api,
    handle: me,
    deviceId: idA,
    accountId: ACCOUNT,
    credential: 'mwd_test',
    events: (event) => events.push(event.type),
    openSocket: () => transport.a(),
  })
  const machinePeer = new WebPeer({
    api: 'http://relay.test',
    wasm: api,
    handle: machine,
    deviceId: idB,
    accountId: ACCOUNT,
    credential: 'mwd_test',
    events: (event) => events.push(event.type),
    openSocket: () => transport.b(),
  })
  client.knowDevices([{ id: idB, noise_pubkey: base64url(api.pub(machine)) }])
  machinePeer.knowDevices([{ id: idA, noise_pubkey: base64url(api.pub(me)) }])
  await client.connect()
  await machinePeer.connect()
  // The machine's own side of a request: the daemon's shape, without a daemon.
  machinePeer.rpc.set(idA, {
    onMessage(bytes) {
      const parsed = parse(bytes)
      if (!parsed || parsed[0] !== 0x10) return
      const [kind, stream] = parsed
      const body = encoder.encode('{"v":1,"sessions":[]}')
      machinePeer.sendSealed(
        idA,
        frame(HEAD, stream, encoder.encode(JSON.stringify({ status: 200, content_type: 'application/json' }))),
      )
      machinePeer.sendSealed(idA, frame(RDATA, stream, body))
      machinePeer.sendSealed(idA, frame(REND, stream, new Uint8Array(0)))
    },
  })
  return { api, client, machinePeer, idA, idB, frames, events }
}

test('a browser opens the handshake itself before it asks anything', async () => {
  const { client, idB, frames, events } = await socketPair()
  const answer = await client.fetchTo(idB)('http://link.openremote.invalid/sync/summary')
  assert.equal(answer.status, 200)
  assert.deepEqual(JSON.parse(await answer.text()), { v: 1, sessions: [] })
  // The first frame on the wire is a handshake, not a sealed request: a
  // request sealed before the link exists has nowhere to go.
  assert.equal(frames[0][0], 1, 'the lower id opens with a handshake')
  assert.ok(events.includes('up'), 'the link is reported up')
})

test('the higher id nudges, and the lower one starts the handshake', async () => {
  const { client, idB, frames, events } = await socketPair({ swap: true })
  const answer = await client.fetchTo(idB)('http://link.openremote.invalid/sync/summary')
  assert.equal(answer.status, 200, 'the call still crosses')
  // The nudge is a bare type byte, unsealed, because there is no link yet.
  assert.deepEqual(frames.find((one) => one.length === 1), [4], 'a nudge went out first')
  assert.ok(events.filter((one) => one === 'up').length >= 2, 'both sides came up')
})

test('a second invitation never restarts the handshake it is waiting for', async () => {
  const { client, idB, frames } = await socketPair()
  // The relay can deliver a nudge while a handshake is already on its way, and
  // answering it with a second start would throw that handshake away.
  client.invite(idB)
  const handshakes = frames.filter((one) => one[0] === 1).length
  assert.equal(handshakes, 1, 'one start, not two')
  const answer = await client.fetchTo(idB)('http://link.openremote.invalid/sync/summary')
  assert.equal(answer.status, 200, 'and the link still comes up')
})

test('a stale handshake answer does not claim a link this side does not have', async () => {
  const { client, idB, events } = await socketPair()
  client.plaintext(idB, new Uint8Array([2, 9, 9, 9]))
  assert.ok(!events.includes('up'), 'nothing came up')
  assert.equal(client.links.get(idB), undefined, 'and no link is recorded')
})

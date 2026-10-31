// The relay socket a browser may open, and the small request layer over it.
//
// A browser's WebSocket cannot set an Authorization header, so the device
// credential travels in the subprotocol (`cred.<credential>`); the gateway
// accepts it there as well as in the header the daemons send, and echoes the
// selected subprotocol back. Text frames are the relay's control messages;
// binary frames are `[16-byte destination][payload]` and, coming in, their
// destination becomes the sender's id.
//
// Nothing here decrypts: a payload goes to and comes from the wasm link, which
// is where every protocol byte is handled. The one rule this file must keep
// straight is which payloads are sealed: a handshake (1 and 2) and a nudge (4)
// travel as they are, because there is no link yet to seal them with, and only
// a request frame (3) is sealed.

import { Rpc, shaped } from './rpc.js'

export const CLOSE_REPLACED = 4001
export const CLOSE_REVOKED = 4003
export const CLOSE_ROTATED = 4005

const HS1 = 1
const HS2 = 2
const MSG = 3
const NUDGE = 4

/** How long one device gets to answer a handshake before the caller is told. */
export const LINK_TIMEOUT_MS = 12000

/** A socket to the account's relay, authenticating with the credential. */
export function openRelay({ api, credential }) {
  const url = `${api.replace(/^http/, 'ws').replace(/\/+$/, '')}/v1/openremote/relay`
  const socket = new WebSocket(url, [`cred.${credential}`])
  socket.binaryType = 'arraybuffer'
  return socket
}

/**
 * One browser device as a peer on the mesh: its own wasm handle, the socket,
 * and the calls the console makes. `events` is called with what happened so the
 * caller can show presence and device changes; `openSocket` is the transport,
 * injectable so a test can wire two peers to each other without a network.
 */
export class WebPeer {
  constructor({ api, wasm, handle, deviceId, accountId, credential, events = () => {}, openSocket = openRelay }) {
    this.api = api
    this.wasm = wasm
    this.handle = handle
    this.deviceId = deviceId
    this.accountId = accountId
    this.credential = credential
    this.events = events
    this.openSocket = openSocket
    this.peers = new Map() // device id text -> public key bytes
    this.links = new Map() // device id text -> 'up'
    this.rpc = new Map() // device id text -> Rpc
    this.waiting = new Map() // device id text -> { resolve, reject, timer }
    this.socket = null
    // Devices this side is mid-handshake with: one attempt per peer at a time,
    // because a second one throws away the handshake the first is waiting for.
    this.linking = new Set()
  }

  /** Opens the socket and starts answering its control frames. */
  connect() {
    return new Promise((resolve, reject) => {
      const socket = this.openSocket({ api: this.api, credential: this.credential })
      this.socket = socket
      socket.onerror = () => reject(new Error('the relay socket failed'))
      socket.onclose = (event) => {
        this.closed(event)
      }
      // A test transport may already be open; the real one is not.
      if (typeof socket.readyState === 'number' && socket.readyState === 1) resolve()
      else socket.onopen = () => resolve()
      socket.onmessage = (event) => this.onMessage(event.data)
    })
  }

  /** The socket went away: nothing can be answered on it any more. */
  closed(event = {}) {
    this.links.clear()
    this.linking.clear()
    for (const peer of [...this.waiting.keys()]) this.refuse(peer, new Error('the relay closed'))
    this.events({ type: 'closed', code: event.code ?? 0, reason: event.reason ?? '' })
  }

  /** Everything not addressed to this peer's own id is the relay talking. */
  onMessage(data) {
    if (typeof data === 'string') {
      this.onControl(safeJson(data))
      return
    }
    const bytes = new Uint8Array(data)
    if (bytes.length < 17) return
    // The relay carries a device id as its 16 bytes; every map in this class
    // is keyed by the text form the registry uses, so a frame names the same
    // device as `knowDevices` did.
    const from = uuidOf(bytes.subarray(0, 16))
    this.plaintext(from, bytes.subarray(16))
  }

  /** The relay's text protocol: hello, presence, devices_changed, revoked,
   *  undeliverable, error. */
  onControl(message) {
    if (!message) return
    switch (message.type) {
      case 'hello':
        for (const id of message.devices ?? []) this.events({ type: 'presence', id, online: true })
        break
      case 'presence':
        this.events({ type: 'presence', id: message.id, online: Boolean(message.online) })
        break
      case 'devices_changed':
        // The device list moved: the caller reloads it, the relay does not
        // carry the keys (it never sees them).
        this.events({ type: 'devices_changed' })
        break
      case 'revoked':
        this.events({ type: 'revoked' })
        break
      case 'undeliverable':
        this.refuse(message.id, new Error('that device is not online'))
        this.events({ type: 'undeliverable', id: message.id })
        break
      case 'error':
        this.events({ type: 'error', message: String(message.error ?? message.message ?? '') })
        break
      default:
        break
    }
  }

  /** A payload from one peer: a handshake step, or a request frame. */
  plaintext(from, payload) {
    const kind = payload[0]
    if (kind === HS1) {
      // A handshake from a peer this side did not start: answer it, if the
      // peer's public key is known from the registry. The answer travels
      // unsealed, because this is what makes a link exist.
      const key = this.peers.get(from)
      if (!key) return
      let hs2
      try {
        hs2 = this.wasm.respond(this.handle, key, this.accountId, idBytes(this.deviceId), idBytes(from), payload)
      } catch {
        return
      }
      this.send(from, hs2)
      this.up(from)
      return
    }
    if (kind === HS2) {
      let up = false
      try {
        up = this.wasm.complete(this.handle, payload)
      } catch {
        up = false
      }
      // A stale answer to a handshake that has been replaced is not a link:
      // reporting one here would leave every later request sealed onto nothing.
      if (up) this.up(from)
      return
    }
    if (kind === NUDGE) {
      // The higher id asks the lower one to start. Answering means starting,
      // and only the side that is meant to initiate may do so; a nudge that
      // arrives while that handshake is already under way changes nothing.
      if (!this.linking.has(from) && this.before(from)) this.invite(from)
      return
    }
    // A request frame is the only payload the link seals, so this is the one
    // place the socket opens anything.
    let opened
    try {
      opened = this.wasm.open(this.handle, payload)
    } catch {
      // A frame this link cannot open is dropped, not answered: the peer will
      // retry or re-handshake, and the relay is blind either way.
      return
    }
    const rpc = this.rpc.get(from)
    if (rpc) rpc.onMessage(opened)
  }

  up(from) {
    if (this.links.get(from) === 'up') return
    this.links.set(from, 'up')
    this.linking.delete(from)
    this.answer(from)
    this.events({ type: 'up', id: from })
  }

  /** One relay frame as it is: `[destination][payload]`. */
  send(to, payload) {
    if (!this.socket || this.socket.readyState !== 1) {
      throw new Error('the relay socket is not open')
    }
    const out = new Uint8Array(16 + payload.length)
    out.set(idBytes(to), 0)
    out.set(payload, 16)
    this.socket.send(out)
  }

  /** One relay frame with its payload sealed by the link. */
  sendSealed(to, plaintext) {
    this.send(to, this.wasm.seal(this.handle, plaintext))
  }

  /** Registers the account's other devices' public keys, as the registry
   *  answered them. */
  knowDevices(devices) {
    for (const device of devices) {
      if (device.id === this.deviceId) continue
      this.peers.set(device.id, base64urlToBytes(device.noise_pubkey ?? ''))
    }
  }

  /** Starts a link to `peer`: this side initiates when its id is the lower. */
  invite(peer) {
    if (this.links.get(peer) === 'up' || this.linking.has(peer)) return
    const key = this.peers.get(peer)
    if (!key) throw new Error('that device has no key on this account yet')
    this.linking.add(peer)
    if (this.before(peer)) {
      try {
        this.send(peer, this.wasm.initiate(this.handle, key, this.accountId, idBytes(this.deviceId), idBytes(peer)))
      } catch (error) {
        this.linking.delete(peer)
        throw error
      }
    } else {
      // The higher id nudges the lower one to initiate, so two devices never
      // start a handshake at the same moment. A nudge is not sealed: there is
      // no link to seal it with.
      this.send(peer, new Uint8Array([NUDGE]))
    }
  }

  /** The device with the lower id initiates; ids are compared as bytes. */
  before(peer) {
    const mine = idBytes(this.deviceId)
    const theirs = idBytes(peer)
    for (let at = 0; at < 16; at += 1) {
      if (mine[at] !== theirs[at]) return mine[at] < theirs[at]
    }
    return false
  }

  /** Resolves once the link to `peer` is up, starting one if there is none. */
  linkTo(peer, timeoutMs = LINK_TIMEOUT_MS) {
    if (this.links.get(peer) === 'up') return Promise.resolve()
    const pending = this.waiting.get(peer)
    if (pending) return pending.promise
    let resolve, reject
    const promise = new Promise((yes, no) => {
      resolve = yes
      reject = no
    })
    const timer = setTimeout(() => this.refuse(peer, new Error('that device did not answer')), timeoutMs)
    this.waiting.set(peer, { resolve, reject, timer, promise })
    try {
      this.invite(peer)
    } catch (error) {
      this.refuse(peer, error)
    }
    return promise
  }

  answer(peer) {
    const pending = this.waiting.get(peer)
    if (!pending) return
    clearTimeout(pending.timer)
    this.waiting.delete(peer)
    pending.resolve()
  }

  refuse(peer, error) {
    const pending = this.waiting.get(peer)
    this.linking.delete(peer)
    if (!pending) return
    clearTimeout(pending.timer)
    this.waiting.delete(peer)
    pending.reject(error)
  }

  /** The request layer to `peer`, once the link is up. */
  rpcTo(peer) {
    const existing = this.rpc.get(peer)
    if (existing) return existing
    const rpc = new Rpc({ send: (bytes) => this.sendSealed(peer, bytes) })
    this.rpc.set(peer, rpc)
    return rpc
  }

  /**
   * One call over the link: `call(peer, method, path, body)` answers
   * `{status, contentType, body}`. Opens the link first when it is not up,
   * because a request sealed before the handshake has nowhere to go.
   */
  async call(peer, method, path, body = null, options = {}) {
    await this.linkTo(peer)
    return this.rpcTo(peer).call(method, path, body, options)
  }

  fetchTo(peer) {
    return async (url, init = {}) => {
      const parsed = new URL(url, 'http://daemon')
      const method = (init.method ?? 'GET').toUpperCase()
      const answer = await this.call(peer, method, `${parsed.pathname}${parsed.search}`, init.body ?? null, {
        signal: init.signal ?? null,
      })
      return shaped(answer)
    }
  }

  close() {
    this.socket?.close()
    this.socket = null
  }
}

function safeJson(text) {
  try {
    return JSON.parse(text)
  } catch {
    return null
  }
}

/** A device id as the 16 bytes the prologue takes. */
export function idBytes(id) {
  const clean = String(id).replace(/-/g, '')
  const out = new Uint8Array(16)
  for (let at = 0; at < 16; at += 1) out[at] = parseInt(clean.slice(at * 2, at * 2 + 2), 16)
  return out
}

/** The same 16 bytes as the text form the registry and every map here use. */
export function uuidOf(bytes) {
  const text = hex(bytes)
  return `${text.slice(0, 8)}-${text.slice(8, 12)}-${text.slice(12, 16)}-${text.slice(16, 20)}-${text.slice(20)}`
}

export function hex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function base64urlToBytes(value) {
  const padded = value.replace(/-/g, '+').replace(/_/g, '/')
  const binary = atob(padded + '='.repeat((4 - (padded.length % 4)) % 4))
  return Uint8Array.from(binary, (char) => char.charCodeAt(0))
}

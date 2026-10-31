// The WebAssembly side of the browser peer: loading the module and the small
// set of calls the ABI offers, with the memory handling out of the way.
//
// The module is a plain cdylib: no wasm-bindgen, one import (the page's random
// bytes) and a dozen exports. Everything here is about pointers and lengths;
// the protocol lives in Rust (see daemon/crates/openremote-noise).

/** The ABI's error codes, as the Rust side numbers them. */
export const ERRORS = {
  0: null,
  1: 'the bytes are not a usable key',
  2: 'the link is not in the state this step needs',
  3: 'the handshake refused this peer',
  4: 'the frame was not one this step accepts',
  5: 'a panic was caught at the boundary',
}

/** Source of the module's one random byte call: the page's own crypto. */
function randomFill(ptr, len) {
  const bytes = new Uint8Array(len)
  globalThis.crypto.getRandomValues(bytes)
  // `wasm` is closed over when the module is instantiated; see loadWasm.
  const heap = new Uint8Array(wasm.exports.memory.buffer, ptr, len)
  heap.set(bytes)
}

let wasm = null

/**
 * Loads the module from bytes or a URL. `source` is an ArrayBuffer, a
 * Uint8Array, or a URL string (the page fetches the file).
 */
export async function loadWasm(source) {
  let bytes = source
  if (typeof source === 'string') {
    const response = await fetch(source)
    if (!response.ok) throw new Error(`the web peer module did not load: HTTP ${response.status}`)
    bytes = await response.arrayBuffer()
  }
  const { instance } = await WebAssembly.instantiate(bytes, {
    env: { wp_random_fill: randomFill },
  })
  wasm = instance
  return wrap(instance)
}

function wrap(instance) {
  const e = instance.exports
  const mem = () => new Uint8Array(e.memory.buffer)
  const api = {
    alloc: (len) => e.wp_alloc(len) >>> 0,
    free: (ptr, len) => e.wp_free(ptr, len),
    len: () => e.wp_len() >>> 0,
    error: () => e.wp_error(),
    errorText: () => ERRORS[e.wp_error()] ?? 'the web peer module failed',
    /** Writes bytes into linear memory and answers the pointer. */
    put(bytes) {
      const ptr = api.alloc(bytes.length)
      mem().set(bytes, ptr)
      return ptr
    },
    /** Copies a returned buffer out of linear memory before the next call. */
    take(ptr) {
      const len = api.len()
      if (!ptr) return new Uint8Array(0)
      return mem().slice(ptr, ptr + len)
    },
    pub(handle) {
      return api.take(e.wp_pubkey(handle))
    },
    newPeer(privateKey) {
      return e.wp_new(api.put(privateKey)) >>> 0
    },
    initiate(handle, peerPublic, account, selfId, peerId) {
      return checked(api,
        e.wp_initiate(
          handle,
          api.put(peerPublic),
          api.put(new TextEncoder().encode(account)),
          account.length,
          api.put(selfId),
          api.put(peerId),
        ),
      )
    },
    respond(handle, peerPublic, account, selfId, peerId, hs1) {
      return checked(api,
        e.wp_respond(
          handle,
          api.put(peerPublic),
          api.put(new TextEncoder().encode(account)),
          account.length,
          api.put(selfId),
          api.put(peerId),
          api.put(hs1),
          hs1.length,
        ),
      )
    },
    complete(handle, hs2) {
      return e.wp_complete(handle, api.put(hs2), hs2.length) === 0
    },
    seal(handle, bytes) {
      const ptr = e.wp_seal(handle, api.put(bytes), bytes.length)
      if (!ptr && api.error() !== 0) throw new Error(api.errorText())
      return api.take(ptr)
    },
    /** Opens one relay payload. Throws rather than answering empty. */
    open(handle, frame) {
      const ptr = e.wp_open(handle, api.put(frame), frame.length)
      if (api.error() !== 0) throw new Error(api.errorText())
      if (!ptr) throw new Error('the frame opened to nothing')
      return api.take(ptr)
    },
  }
  return api
}

/** The `crypto.getRandomValues` bytes the wasm handshake needs. Exported for
 *  the identity module, which mints the device's private key the same way. */
export function randomKey() {
  const key = new Uint8Array(32)
  globalThis.crypto.getRandomValues(key)
  return key
}

/** A call that answers bytes: its error, when it set one, is the answer. */
function checked(api, ptr) {
  if (api.error() !== 0) throw new Error(api.errorText())
  return api.take(ptr)
}

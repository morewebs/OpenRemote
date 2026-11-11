// Request streams over the encrypted link, and a `fetch`-shaped front for them.
//
// The framing is the daemon's (`daemon/crates/openremote-cloud/src/rpc.rs`):
// `[kind][stream id, u32 big-endian][body]`, with the request's JSON header in
// OPEN, its body in DATA chunks, END closing it, and HEAD/RDATA/REND carrying
// the answer. Several calls run at once, each with its own stream id.
//
// Nothing here is crypto: the caller hands whole plaintext frames to `send`, and
// the transport seals them. That keeps this module testable without a socket
// and without the wasm module.

export const OPEN = 0x10
export const DATA = 0x11
export const END = 0x12
export const HEAD = 0x13
export const RDATA = 0x14
export const REND = 0x15
export const RESET = 0x16
export const NOTE = 0x20

/** Body bytes per frame, well inside one Noise message. */
export const CHUNK = 48 * 1024
export const MAX_REQUEST = 4 << 20
export const MAX_RESPONSE = 16 << 20

const encoder = new TextEncoder()
const decoder = new TextDecoder()

/** One frame, ready for the link. */
export function frame(kind, stream, body = new Uint8Array(0)) {
  const out = new Uint8Array(5 + body.length)
  out[0] = kind
  out[1] = (stream >>> 24) & 0xff
  out[2] = (stream >>> 16) & 0xff
  out[3] = (stream >>> 8) & 0xff
  out[4] = stream & 0xff
  out.set(body, 5)
  return out
}

/** A frame back into its parts, or null when it is too short to be one. */
export function parse(frameBytes) {
  if (frameBytes.length < 5) return null
  const stream =
    ((frameBytes[1] << 24) | (frameBytes[2] << 16) | (frameBytes[3] << 8) | frameBytes[4]) >>> 0
  return [frameBytes[0], stream, frameBytes.subarray(5)]
}

/** The frames that carry one header and body: the header, the chunks, the end. */
export function frames(headKind, stream, header, body = new Uint8Array(0)) {
  const [data, end] = headKind === OPEN ? [DATA, END] : [RDATA, REND]
  const out = [frame(headKind, stream, header)]
  for (let at = 0; at < body.length; at += CHUNK) {
    out.push(frame(data, stream, body.subarray(at, at + CHUNK)))
  }
  out.push(frame(end, stream, new Uint8Array(0)))
  return out
}

/** An error from the far side, with the status the daemon answered. */
export class RpcError extends Error {
  constructor(message, status = null) {
    super(message)
    this.name = 'RpcError'
    this.status = status
  }
}

/**
 * The streams of one link. `send(frameBytes)` seals and sends one plaintext
 * frame; `receive(frameBytes)` is called with every plaintext frame the link
 * opened. A call resolves with the answer's header and body bytes.
 */
export class Rpc {
  constructor({ send, timeoutMs = 20_000 }) {
    this.send = send
    this.timeoutMs = timeoutMs
    this.streams = new Map()
    this.next = 1
  }

  nextStream() {
    const id = this.next
    this.next = (this.next + 1) >>> 0
    if (this.next === 0) this.next = 1
    return id
  }

  /** A one-way note (stream 0) to the peer. */
  note(value) {
    this.send(frame(NOTE, 0, encoder.encode(JSON.stringify(value))))
  }

  /**
   * One request. `body` is bytes or a string; answers `{status, contentType,
   * body}` with the body as bytes. Rejects on a timeout, on the caller's
   * AbortSignal (a RESET goes out), or when the far side answers a reset.
   */
  call(method, path, body = null, { signal = null, timeoutMs = this.timeoutMs } = {}) {
    const bytes =
      body == null ? new Uint8Array(0) : typeof body === 'string' ? encoder.encode(body) : body
    if (bytes.length > MAX_REQUEST) {
      return Promise.reject(new RpcError('this request is too large to send over the link'))
    }
    const stream = this.nextStream()
    return new Promise((resolve, reject) => {
      if (signal?.aborted) {
        reject(new RpcError('the call was aborted'))
        return
      }
      const timer = timeoutMs
        ? setTimeout(() => {
            this.abort(stream, 'the far side did not answer in time')
          }, timeoutMs)
        : null
      const entry = {
        kind: 'request',
        chunks: [],
        status: null,
        contentType: null,
        resolve,
        reject,
        timer,
        signal,
        onAbort: () => this.abort(stream, 'the call was aborted'),
      }
      this.streams.set(stream, entry)
      if (signal) signal.addEventListener('abort', entry.onAbort, { once: true })
      const header = encoder.encode(
        JSON.stringify({
          method,
          path,
          ...(body == null ? {} : { content_type: 'application/json' }),
        }),
      )
      for (const one of frames(OPEN, stream, header, bytes)) this.send(one)
    })
  }

  /** Gives up on a stream and tells the far side to do the same. */
  abort(stream, why) {
    const entry = this.streams.get(stream)
    if (!entry) return
    this.settle(stream, entry, new RpcError(why))
    this.send(frame(RESET, stream, encoder.encode(JSON.stringify({ error: why }))))
  }

  settle(stream, entry, error, answer) {
    this.streams.delete(stream)
    if (entry.timer) clearTimeout(entry.timer)
    if (entry.signal) entry.signal.removeEventListener('abort', entry.onAbort)
    if (error) entry.reject(error)
    else entry.resolve(answer)
  }

  /** Rebuilds a response body out of the RDATA chunks. */
  onMessage(frameBytes) {
    const parsed = parse(frameBytes)
    if (!parsed) return
    const [kind, stream, body] = parsed
    const entry = this.streams.get(stream)
    if (!entry) return
    if (kind === RESET) {
      const said = safeJson(body)
      this.settle(stream, entry, new RpcError(said?.error ?? 'the far side reset this call'))
      return
    }
    if (kind !== HEAD && kind !== RDATA && kind !== REND) return
    if (kind === HEAD) {
      const head = safeJson(body) ?? {}
      entry.status = Number(head.status ?? 0)
      entry.contentType = head.content_type ?? null
      return
    }
    if (kind === RDATA) {
      entry.chunks.push(body)
      const total = entry.chunks.reduce((sum, chunk) => sum + chunk.length, 0)
      if (total > MAX_RESPONSE) {
        this.settle(stream, entry, new RpcError('the answer is larger than one call may carry'))
      }
      return
    }
    this.settle(stream, entry, null, {
      status: entry.status,
      contentType: entry.contentType,
      body: concat(entry.chunks),
    })
  }
}

function concat(chunks) {
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0)
  const out = new Uint8Array(total)
  let at = 0
  for (const chunk of chunks) {
    out.set(chunk, at)
    at += chunk.length
  }
  return out
}

function safeJson(bytes) {
  try {
    return JSON.parse(decoder.decode(bytes))
  } catch {
    return null
  }
}

/**
 * `fetchShaped(url, init)` over `call`, so the console's one `fetch` can be
 * swapped for it without any other change. The URL is the daemon-shaped address
 * the console already builds; only its path and query travel.
 */
export function fetchShaped(rpc, base = 'http://daemon') {
  return async function webFetch(url, init = {}) {
    const parsed = new URL(url, base)
    const method = (init.method ?? 'GET').toUpperCase()
    const body = init.body ?? null
    const answer = await rpc.call(`${method} ${parsed.pathname}`, '', null, {
      signal: init.signal ?? null,
    })
    return shaped(answer)
  }
}

/** One answer in the shape `fetch` gives, for a `call` that already ran. */
export function shaped(answer) {
  const text = () => Promise.resolve(new TextDecoder().decode(answer.body))
  return {
    ok: answer.status >= 200 && answer.status < 300,
    status: answer.status,
    headers: {
      get: (name) => (String(name).toLowerCase() === 'content-type' ? answer.contentType : null),
    },
    text,
    json: () => text().then((value) => JSON.parse(value)),
    // A picture: the API client's blob(sha) reads it, typed by the answer.
    blob: () => Promise.resolve(new Blob([answer.body], { type: answer.contentType ?? '' })),
  }
}

export { encoder as textEncoder, decoder as textDecoder }

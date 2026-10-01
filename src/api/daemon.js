// The daemon API client. Every mutating call carries a client request_id
// (the receipt contract: retries dedup; crashes surface as unknown, never
// resent). SSE subscriptions resume from a cursor.

const TOKEN_KEY = 'openremote-daemon-token'
const URL_KEY = 'openremote-daemon-url'

export function loadConnection() {
  try {
    return { url: localStorage.getItem(URL_KEY), token: localStorage.getItem(TOKEN_KEY) }
  } catch {
    return { url: null, token: null }
  }
}

export function saveConnection(url, token) {
  try {
    if (url) localStorage.setItem(URL_KEY, url)
    if (token) localStorage.setItem(TOKEN_KEY, token)
  } catch {
    /* storage unavailable */
  }
}

export function clearConnection() {
  try {
    localStorage.removeItem(URL_KEY)
    localStorage.removeItem(TOKEN_KEY)
  } catch {
    /* storage unavailable */
  }
}

export function newRequestId() {
  return globalThis.crypto?.randomUUID?.() ?? `req-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

export class DaemonApi {
  constructor(url, token) {
    this.url = url?.replace(/\/+$/, '') ?? null
    this.token = token
  }

  get ready() {
    return Boolean(this.url && this.token)
  }

  async call(method, path, body) {
    const response = await fetch(`${this.url}${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${this.token}`,
        ...(body ? { 'Content-Type': 'application/json' } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    })
    const text = await response.text()
    let json = null
    try {
      json = text ? JSON.parse(text) : null
    } catch {
      json = null
    }
    if (!response.ok) {
      const error = new Error(json?.error ?? `HTTP ${response.status}`)
      error.status = response.status
      throw error
    }
    return json
  }

  health() {
    return this.call('GET', '/healthz')
  }

  capabilities() {
    return this.call('GET', '/capabilities')
  }

  sessions() {
    return this.call('GET', '/sessions')
  }

  session(id) {
    return this.call('GET', `/sessions/${id}`)
  }

  createSession({ harness, workspace, model, permissionMode }) {
    return this.call('POST', '/sessions', {
      request_id: newRequestId(),
      harness,
      workspace,
      ...(model ? { model } : {}),
      ...(permissionMode ? { permission_mode: permissionMode } : {}),
    })
  }

  prompt(id, text) {
    return this.call('POST', `/sessions/${id}/prompts`, { request_id: newRequestId(), text })
  }

  stop(id) {
    return this.call('POST', `/sessions/${id}/stop`, { request_id: newRequestId() })
  }

  resume(id) {
    return this.call('POST', `/sessions/${id}/resume`, { request_id: newRequestId() })
  }

  decisions(id) {
    return this.call('GET', `/sessions/${id}/decisions`)
  }

  answer(decisionId, choice) {
    return this.call('POST', `/decisions/${decisionId}/answer`, {
      request_id: newRequestId(),
      choice,
    })
  }

  // The live event stream for one session. `onEvent` receives each parsed
  // event; the reader keeps going until the connection drops, then reports
  // the last cursor through `onClose(cursor)` so the caller can resume with
  // `after`. Returns a stop function.
  events(id, { after = null, onEvent, onClose }) {
    const controller = new AbortController()
    const run = async () => {
      let cursor = after
      try {
        const query = cursor == null ? '' : `?after=${cursor}`
        const response = await fetch(`${this.url}/sessions/${id}/events${query}`, {
          headers: { Authorization: `Bearer ${this.token}` },
          signal: controller.signal,
        })
        if (!response.ok || !response.body) throw new Error(`SSE HTTP ${response.status}`)
        const reader = response.body.getReader()
        const decoder = new TextDecoder()
        let buffer = ''
        let lastId = cursor
        while (true) {
          const { done, value } = await reader.read()
          if (done) break
          buffer += decoder.decode(value, { stream: true })
          let index
          while ((index = buffer.indexOf('\n')) >= 0) {
            const line = buffer.slice(0, index).replace(/\r$/, '')
            buffer = buffer.slice(index + 1)
            if (line.startsWith('id: ')) {
              lastId = Number.parseInt(line.slice(4), 10)
            } else if (line.startsWith('data: ')) {
              const event = JSON.parse(line.slice(6))
              cursor = event.seq
              onEvent(event)
            }
          }
        }
        onClose?.(lastId)
      } catch (err) {
        if (err?.name !== 'AbortError') onClose?.(lastId ?? cursor)
      }
    }
    run()
    return () => controller.abort()
  }
}

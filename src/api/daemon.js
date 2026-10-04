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
    let clean = url?.replace(/\/+$/, '') ?? null
    // Manual entry often omits the scheme; without one, fetch treats the
    // address as a relative path and fails confusingly.
    if (clean && !/^https?:\/\//.test(clean)) clean = `http://${clean}`
    this.url = clean
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

  createSession({ harness, workspace, model, permissionMode, fast }) {
    return this.call('POST', '/sessions', {
      request_id: newRequestId(),
      harness,
      workspace,
      ...(model ? { model } : {}),
      ...(permissionMode ? { permission_mode: permissionMode } : {}),
      ...(fast ? { fast: true } : {}),
    })
  }

  prompt(id, text) {
    return this.call('POST', `/sessions/${id}/prompts`, { request_id: newRequestId(), text })
  }

  // Model, effort, and fast on a live chat. A harness that only accepts the
  // change when the chat starts answers 409 — the composer says so.
  updateSettings(id, { model, effort, fast }) {
    return this.call('POST', `/sessions/${id}/settings`, {
      request_id: newRequestId(),
      ...(model ? { model } : {}),
      ...(effort ? { effort } : {}),
      ...(fast != null ? { fast } : {}),
    })
  }

  stop(id) {
    return this.call('POST', `/sessions/${id}/stop`, { request_id: newRequestId() })
  }

  // Interrupt the running turn — the harness cancels, the session stays
  // alive for the next message. Unlike stop, nothing dies.
  interrupt(id) {
    return this.call('POST', `/sessions/${id}/interrupt`, { request_id: newRequestId() })
  }

  resume(id) {
    return this.call('POST', `/sessions/${id}/resume`, { request_id: newRequestId() })
  }

  decisions(id) {
    return this.call('GET', `/sessions/${id}/decisions`)
  }

  // ---- machines ----

  machines() {
    return this.call('GET', '/machines')
  }

  createMachine({ name, platform }) {
    return this.call('POST', '/machines', { request_id: newRequestId(), name, platform })
  }

  removeMachine(id) {
    return this.call('DELETE', `/machines/${id}?request_id=${encodeURIComponent(newRequestId())}`)
  }

  // The long one: npm runs minutes — the receipt carries the wait.
  installHarness(machineId, harnessId) {
    return this.call('POST', `/machines/${machineId}/harnesses`, {
      request_id: newRequestId(),
      harness: harnessId,
    })
  }

  // ---- plugins ----

  plugins() {
    return this.call('GET', '/plugins')
  }

  pluginMarketplace() {
    return this.call('GET', '/plugins/marketplace')
  }

  installPlugin(machineId, { catalogId, name, detail, command, needsKey }) {
    return this.call('POST', '/plugins', {
      request_id: newRequestId(),
      machine: machineId,
      ...(catalogId ? { catalog_id: catalogId } : { name, detail, command, needs_key: needsKey }),
    })
  }

  removePlugin(pluginId) {
    return this.call('DELETE', `/plugins/${pluginId}?request_id=${encodeURIComponent(newRequestId())}`)
  }

  setPluginEnabled(pluginId, enabled) {
    return this.call('POST', `/plugins/${pluginId}/enabled`, {
      request_id: newRequestId(),
      enabled,
    })
  }

  acknowledgePluginKey(pluginId) {
    return this.call('POST', `/plugins/${pluginId}/key`, { request_id: newRequestId() })
  }

  // ---- automations ----

  automations() {
    return this.call('GET', '/automations')
  }

  // `id` on the body edits; its absence creates.
  saveRule(rule) {
    return this.call('POST', '/automations', {
      request_id: newRequestId(),
      name: rule.name,
      trigger: rule.trigger,
      harness: rule.harness,
      ...(rule.model ? { model: rule.model } : {}),
      workspace: rule.workspace,
      machine: rule.machine,
      task: rule.task,
      ...(rule.id ? { id: rule.id, enabled: rule.enabled } : {}),
    })
  }

  removeRule(ruleId) {
    return this.call(
      'DELETE',
      `/automations/${ruleId}?request_id=${encodeURIComponent(newRequestId())}`,
    )
  }

  setRuleEnabled(ruleId, enabled) {
    return this.call('POST', `/automations/${ruleId}/enabled`, {
      request_id: newRequestId(),
      enabled,
    })
  }

  runRule(ruleId) {
    return this.call('POST', `/automations/${ruleId}/run`, { request_id: newRequestId() })
  }

  // The models a harness advertises, in its own words (empty = the slot
  // stays reserved for that harness).
  models(harnessId) {
    return this.call('GET', `/harnesses/${harnessId}/models`)
  }

  // ---- harness sign-in ----

  // Start the harness's own login command, relayed: the returned view is
  // the first beat; poll signInView for the CLI's words, feed the human's
  // answers through signInInput.
  startSignIn(harnessId) {
    return this.call('POST', `/harnesses/${harnessId}/signin`, {
      request_id: newRequestId(),
    })
  }

  signInView(harnessId) {
    return this.call('GET', `/harnesses/${harnessId}/signin`)
  }

  signInInput(harnessId, text) {
    return this.call('POST', `/harnesses/${harnessId}/signin/input`, {
      request_id: newRequestId(),
      text,
    })
  }

  // Stop a running sign-in relay — the abandoned-browser-flow answer.
  stopSignIn(harnessId) {
    return this.call('POST', `/harnesses/${harnessId}/signin/stop`, {
      request_id: newRequestId(),
    })
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
      let lastId = after
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

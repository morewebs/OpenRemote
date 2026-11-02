// The daemon API client. Every mutating call carries a client request_id
// (the receipt contract: retries dedup; crashes surface as unknown, never
// resent). SSE subscriptions resume from a cursor. Every call but the
// installer, a plugin install and the event stream carries a timeout, so
// a daemon that is wedged (alive, but not answering) surfaces as an error
// instead of a hang.

import { seg } from '../shape.js'
import { TOO_LONG_SENTENCE } from '../send-bound.js'

const TOKEN_KEY = 'openremote-daemon-token'
const URL_KEY = 'openremote-daemon-url'

// A normal request must answer within this; a daemon that is alive but
// stuck holds every spinner forever without it. The installer is exempt
// (it runs minutes by design), a plugin install is exempt for the same
// reason (a marketplace entry can be made by running a command), and so
// is the SSE reader (its lifetime is the stream).
const CALL_TIMEOUT_MS = 10000

// Calls whose wait includes a harness boot: creating or resuming a chat
// spawns the harness CLI and waits for its first answer (opencode's serve
// line is capped at 30 s, pi's first RPC at 60 s, claude's model handshake
// at 30 s), so the normal budget would cut a slow cold boot in half and
// a retry would land as a second chat. Well above every daemon-side boot
// cap, still bounded: the poll's own banner (three missed 10 s beats)
// says "not answering" long before this fires.
const BOOT_TIMEOUT_MS = 120000

// The installer waits: a harness install can run for minutes by design, and the
// daemon holds its own limit (ten minutes). This client had no bound at all on it,
// so a daemon that was alive but wedged left the install spinner going with no way
// out but a cancel that not every caller passes. Same ceiling, one second of slack.
const INSTALL_TIMEOUT_MS = 601000

/// The failure of a request that never answered, as one object with the fields the
/// rest of this file's errors carry: `status` 0 (nothing came back) and a `code` the
/// copy can key on. Thrown as a bare Error before this, so a caller that branches on
/// `err.status` to decide "retryable?" or "the daemon's refusal?" saw `undefined`
/// for the one failure the console wrote itself.
function timedOut() {
  const error = new Error('The daemon did not answer in time')
  error.status = 0
  error.code = 'timeout'
  return error
}

// Calls that leave this machine: a cloud sign-out, a device kind change or
// removal (the registry, over the network), and an enrolment mint. They are
// not the installer, but they are network-shaped and can outlive the ten
// seconds a local read deserves. Bounded, never unlimited: a leave that lands
// after the client has given up reads as a failure, and the retry then
// answers 404, so a person believes they are still signed in when they are
// not.
const CLOUD_TIMEOUT_MS = 60000

// The resume cursor of an SSE stream: the highest event seq the client
// already holds. Only a non-negative integer counts (a digit string too,
// since a cursor can survive a round trip through storage or a URL) -
// anything else is "no cursor", one deliberate full replay, so a garbage
// value can never pose as a position and starve the stream of events.
export function parseCursor(value) {
  if (typeof value === 'number') return Number.isInteger(value) && value >= 0 ? value : null
  if (typeof value === 'string' && /^\d+$/.test(value)) return Number(value)
  return null
}

// One SSE frame, as the protocol defines it: the lines up to the blank line
// that ends it. `id` carries the frame's seq (the daemon writes it first, so
// even a frame whose data cannot be parsed has its position in hand) and
// `data` carries JSON, which the protocol allows to arrive as several lines
// joined by a newline before parsing. A line is `field: value` with one
// optional space after the colon, and a trailing carriage return belongs to
// the line ending, not the payload - both are things a hand-written sender
// does, and either one silently emptied a chat when the reader took a frame
// to be one `data: ` line. Returns `{ seq }` for a frame with an id and no
// data, `{ seq, event }` for a well-formed one, `{ seq, malformed: true }`
// for data that is not JSON, and null for a frame that carries nothing this
// reader can use (a separator or a keep-alive).
export function parseSseFrame(lines) {
  let seq = null
  const data = []
  for (const raw of String(lines ?? '').split('\n')) {
    const line = raw.replace(/\r$/, '')
    // A frame separator, a keep-alive comment (`: ping`), or a line with no
    // field: none of them is a field this reader reads.
    const colon = line.indexOf(':')
    if (colon <= 0) continue
    const field = line.slice(0, colon)
    const value = line.slice(colon + 1).replace(/^ /, '')
    if (field === 'id') {
      const parsed = parseCursor(value)
      if (parsed != null) seq = parsed
    } else if (field === 'data') {
      data.push(value)
    }
  }
  if (!data.length) return seq == null ? null : { seq }
  try {
    return { seq, event: JSON.parse(data.join('\n')) }
  } catch {
    return { seq, malformed: true }
  }
}

// Whether a stream close means the session itself is gone: the daemon
// answered 404 ("no such session"), a chat deleted here or on another
// device. No retry can outlive that - every other close (a dropped
// connection, a wedged daemon, another HTTP error) deserves another
// attempt.
export function sessionGone(err) {
  return err?.status === 404
}

// How long a live stream may carry nothing. The daemon writes a keep-alive
// comment every 15 s, so two missed keep-alives is a half-open socket: the
// reader waits on a body that will never speak again, the chat sits frozen
// mid-turn, and the poll meanwhile answers from memory that the session is
// not running. Any chunk counts, a keep-alive included - the point is that
// the wire is alive, not that it carried an event.
export const KEEP_ALIVE_MS = 15000
export const STREAM_IDLE_MS = KEEP_ALIVE_MS * 2 + 5000

// The re-dial interval after a close: quick at first, because a daemon that
// is restarting comes back in a second or two, then doubling to a cap so a
// machine that is asleep or a daemon that keeps dying is not dialed twice a
// second for as long as the chat stays watched. Takes the delay handed out
// last; 0 or absent means the stream just talked, so the ladder restarts.
export const RETRY_BASE_MS = 800
export const RETRY_CAP_MS = 5000

export function nextRetryDelay(previous) {
  if (previous == null || !(previous > 0)) return RETRY_BASE_MS
  return Math.min(previous * 2, RETRY_CAP_MS)
}

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

// Wire one request's fetch signal: the timeout and the caller's own stop
// each abort one controller, and this function owns both listeners so that
// neither outlives the request. The symmetry is the point - a listener left
// behind holds the whole call alive until its signal eventually fires - so
// it is its own function, exported and tested against a fake pair of
// signals instead of a real timer.
//
// `timeoutSignal` is injectable for that test; production never passes it,
// and the budget remains the only way a timeout is made, which is what the
// request-budget test watches.
export function linkAbort({ timeoutMs = null, signal = null, timeoutSignal = null } = {}) {
  const controller = new AbortController()
  let timedOut = false
  const onTimeout = () => {
    timedOut = true
    controller.abort()
  }
  const timer = timeoutSignal ?? (timeoutMs == null ? null : AbortSignal.timeout(timeoutMs))
  if (timer != null) {
    if (timer.aborted) onTimeout()
    else timer.addEventListener('abort', onTimeout, { once: true })
  }
  const forward = signal == null ? null : () => controller.abort()
  if (signal != null) {
    if (signal.aborted) controller.abort()
    else signal.addEventListener('abort', forward, { once: true })
  }
  return {
    // The signal fetch gets: a browser reads a signal, and a request with
    // neither a budget nor a caller stop asks for none (the installer).
    signal: timer == null && signal == null ? null : controller.signal,
    get timedOut() {
      return timedOut
    },
    // Whatever ended the request, both listeners come off here. One that
    // already fired is off by its own `once`, so this is safe on every path.
    detach() {
      if (timer != null) timer.removeEventListener('abort', onTimeout)
      if (signal != null && forward != null) signal.removeEventListener('abort', forward)
    },
  }
}

// The address the client dials: a trailing slash is not part of it, and a
// manual entry without a scheme is read as http (without one, fetch treats
// the address as a relative path and fails confusingly). One function, so
// the constructor and the comparison below cannot disagree.
export function normaliseUrl(url) {
  let clean = url?.replace(/\/+$/, '') ?? null
  if (clean && !/^https?:\/\//.test(clean)) clean = `http://${clean}`
  return clean
}

/// Whether a status report describes the daemon the console is already
/// pointed at. The shell reports on start, on failure and on a respawn, and
/// a report carrying the same address and token is the same daemon: taking
/// it as a new one would tear down every live stream and replay every
/// transcript to arrive where the console already was.
export function sameDaemon(current, url, token) {
  if (!current?.url) return false
  return current.url === normaliseUrl(url) && current.token === token
}

export class DaemonApi {
  // `prefix` points every call at another of the user's devices, through
  // this daemon (`/devices/<id>`): the same API, answered over there.
  //
  // `fetchImpl` is the one seam the web console needs: in the desktop the
  // address is a loopback port and the global `fetch` dials it, while in a
  // browser the same requests travel over an end-to-end encrypted link. The
  // client never reaches for the global itself, so every request it makes -
  // the ordinary calls and the live event stream - goes through this field.
  //
  // The default is a forwarder rather than the global itself: it reads
  // `globalThis.fetch` when the request is made, so a page (or a test) that
  // swaps the global between calls keeps working, which is the behaviour the
  // desktop has always had and the suite relies on.
  constructor(url, token, prefix = '', fetchImpl = null) {
    this.url = normaliseUrl(url)
    this.token = token
    this.prefix = prefix
    this.fetch =
      typeof fetchImpl === 'function' ? fetchImpl : (target, init) => globalThis.fetch(target, init)
  }

  /** The same client, aimed at another of the user's devices. */
  device(id) {
    return id ? new DaemonApi(this.url, this.token, `/devices/${seg(id)}`, this.fetch) : this
  }

  get ready() {
    return Boolean(this.url && this.token)
  }

  async call(method, path, body, { timeoutMs = CALL_TIMEOUT_MS, signal = null, as = 'json' } = {}) {
    let response
    let text = ''
    // The timeout and the caller's own stop (an unmounting folder
    // picker, say) each end the request, whichever fires first. `linked`
    // owns both listeners and takes them off however the request ends.
    const linked = linkAbort({ timeoutMs, signal })
    try {
      response = await this.fetch(`${this.url}${this.prefix}${path}`, {
        method,
        headers: {
          Authorization: `Bearer ${this.token}`,
          ...(body ? { 'Content-Type': 'application/json' } : {}),
        },
        body: body ? JSON.stringify(body) : undefined,
        // `null` opts out (the calls whose wait is the point); otherwise
        // the request must answer within `timeoutMs`.
        signal: linked.signal ?? undefined,
      })
      text = await response.text()
    } catch (err) {
      // A timed-out request reads as "not answering": the signal aborts
      // both the headers wait and a stalled body read, and callers only
      // have err.message to show - the browser's own "signal timed out"
      // says nothing. A caller's own stop is a deliberate cancel, not a
      // failure: the raw error rides through and the stopper ignores it.
      if (linked.timedOut) {
        throw timedOut()
      }
      if (signal?.aborted) {
        throw err
      }
      if (err?.name === 'TimeoutError' || err?.name === 'AbortError') {
        throw timedOut()
      }
      throw err
    } finally {
      linked.detach()
    }
    let json = null
    let unreadable = false
    try {
      json = text ? JSON.parse(text) : null
    } catch {
      unreadable = true
    }
    if (!response.ok) {
      // A body the daemon refused arrives with nothing to read, so name the
      // limit instead of the code: "HTTP 413" tells a person nothing about
      // what to cut.
      if (response.status === 413) {
        const tooLong = new Error(TOO_LONG_SENTENCE)
        tooLong.status = 413
        throw tooLong
      }
      const error = new Error(json?.error ?? `HTTP ${response.status}`)
      error.status = response.status
      throw error
    }
    // One route answers a document rather than JSON (the third-party
    // notices): same timeout, same abort rules, same error shape, and the
    // caller gets the text it actually reads.
    if (as === 'text') return text
    // A success whose body could not be read is not an empty answer: a proxy that
    // truncates, or a daemon killed mid-body, used to hand the caller `null` where a
    // list was promised, so the pane rendered as empty and said nothing.
    if (unreadable) {
      const error = new Error('The daemon answered, but its answer could not be read')
      error.status = response.status
      throw error
    }
    return json
  }

  health() {
    return this.call('GET', '/healthz')
  }

  // The one route that answers a document rather than JSON: the
  // third-party notices, served as plain text because they are read, not
  // parsed. It rides call() so the timeout, the caller's stop and the
  // error shape are the same ones every other route has.
  notices() {
    return this.call('GET', '/notices', null, { as: 'text' })
  }

  capabilities() {
    return this.call('GET', '/capabilities')
  }

  sessions() {
    return this.call('GET', '/sessions')
  }

  session(id) {
    return this.call('GET', `/sessions/${seg(id)}`)
  }

  // Creating the chat boots the harness (up to 60 s of CLI cold start
  // daemon-side) - the boot budget, not the normal one.
  createSession({ harness, workspace, model, effort, permissionMode, fast, endpointId, deviceId, synced }) {
    return this.call(
      'POST',
      '/sessions',
      {
        request_id: newRequestId(),
        harness,
        workspace,
        ...(model ? { model } : {}),
        ...(effort ? { effort } : {}),
        ...(permissionMode ? { permission_mode: permissionMode } : {}),
        ...(fast ? { fast: true } : {}),
        ...(endpointId ? { endpoint_id: endpointId } : {}),
        ...(deviceId ? { device_id: deviceId } : {}),
        ...(synced ? { synced: true } : {}),
      },
      { timeoutMs: BOOT_TIMEOUT_MS },
    )
  }

  /** Syncs a private chat to the user's other devices (it keeps running here). */
  syncSession(id) {
    return this.call('POST', `/sessions/${seg(id)}/sync`, { request_id: newRequestId() })
  }

  /** Deletes a synced chat on every device. A 404 means the chat is already
   *  gone - a second click, or a delete that landed from another device -
   *  which is the outcome asked for, so it resolves instead of raising an
   *  error over a row that is about to disappear anyway. Every other
   *  answer (a chat that is not synced, a signed-out daemon) stays an
   *  error: those are things the user has to hear. */
  async deleteSession(id) {
    try {
      return await this.call('DELETE', `/sessions/${seg(id)}`)
    } catch (err) {
      if (err?.status === 404) return null
      throw err
    }
  }

  // Cloud mode: this computer's account and devices.

  cloud() {
    return this.call('GET', '/cloud')
  }

  // The receipt contract covers these too: a retry (or a double-firing
  // mount effect) with the same request_id is one mutation, not two.
  // The daemon's cloud routes read what they know and ignore the rest.

  /** Starts a moreweb sign-in; `open` has the daemon open the browser. */
  cloudSignIn({ open = true } = {}) {
    return this.call('POST', '/cloud/signin', { request_id: newRequestId(), open })
  }

  cloudSignInCancel() {
    return this.call('POST', '/cloud/signin/cancel', { request_id: newRequestId() })
  }

  cloudSignOut() {
    return this.call('POST', '/cloud/signout', { request_id: newRequestId() }, { timeoutMs: CLOUD_TIMEOUT_MS })
  }

  cloudDevices() {
    return this.call('GET', '/cloud/devices')
  }

  setDeviceKind(id, kind) {
    return this.call('POST', `/cloud/devices/${seg(id)}/kind`, { request_id: newRequestId(), kind }, { timeoutMs: CLOUD_TIMEOUT_MS })
  }

  removeDevice(id) {
    return this.call('DELETE', `/cloud/devices/${seg(id)}?request_id=${encodeURIComponent(newRequestId())}`, null, { timeoutMs: CLOUD_TIMEOUT_MS })
  }

  createEnrollment() {
    return this.call('POST', '/cloud/enrollments', { request_id: newRequestId() }, { timeoutMs: CLOUD_TIMEOUT_MS })
  }

  /** The subfolders of `path` (default: home) on the device this aims at.
   *  `signal` is the caller's own stop for a navigation that no longer
   *  matters (the folder picker moved on or unmounted). */
  fsDirs(path, { signal = null } = {}) {
    return this.call('GET', `/fs/dirs${path ? `?path=${encodeURIComponent(path)}` : ''}`, null, { signal })
  }

  // `mentionedChats`: the chats this message points at with @ - from now
  // on the chat's agent may read them, and only them.
  prompt(id, text, mentionedChats = []) {
    return this.call('POST', `/sessions/${seg(id)}/prompts`, {
      request_id: newRequestId(),
      text,
      ...(mentionedChats.length ? { mentioned_chats: mentionedChats } : {}),
    })
  }

  // Rename a chat (the composer's /rename).
  rename(id, title) {
    return this.call('POST', `/sessions/${seg(id)}/title`, { request_id: newRequestId(), title })
  }

  // What a composer's / and @ menus offer: a chat's own, or a harness's in
  // a folder before the chat exists. The first ask in a folder starts the
  // harness briefly to list them - the longer budget.
  commands({ harness, workspace, session } = {}) {
    const query = new URLSearchParams()
    if (session) query.set('session', session)
    else {
      if (harness) query.set('harness', harness)
      if (workspace) query.set('workspace', workspace)
    }
    return this.call('GET', `/commands?${query}`, null, { timeoutMs: 60_000 })
  }

  // @ file suggestions in a chat's workspace, or a named folder.
  files({ workspace, session, q = '' } = {}, { signal = null } = {}) {
    const query = new URLSearchParams({ q })
    if (session) query.set('session', session)
    else if (workspace) query.set('workspace', workspace)
    return this.call('GET', `/files?${query}`, null, { signal })
  }

  // Model, effort, and fast on a live chat. A harness that only accepts the
  // change when the chat starts answers 409 - the composer says so. The
  // permission word is the same shape: start-time only, so a live chat's
  // change conflicts and a stopped chat's lands on resume.
  updateSettings(id, { model, effort, fast, permissionMode }) {
    return this.call('POST', `/sessions/${seg(id)}/settings`, {
      request_id: newRequestId(),
      ...(model ? { model } : {}),
      ...(effort ? { effort } : {}),
      ...(fast != null ? { fast } : {}),
      ...(permissionMode ? { permission_mode: permissionMode } : {}),
    })
  }

  stop(id) {
    return this.call('POST', `/sessions/${seg(id)}/stop`, { request_id: newRequestId() })
  }

  // Interrupt the running turn - the harness cancels, the session stays
  // alive for the next message. Unlike stop, nothing dies.
  interrupt(id) {
    return this.call('POST', `/sessions/${seg(id)}/interrupt`, { request_id: newRequestId() })
  }

  // The chat's queue - the daemon keeps it, so every device sees the same
  // one. A queued message runs as its own turn when the running one ends.
  queue(id, text, mentionedChats = []) {
    return this.call('POST', `/sessions/${seg(id)}/queue`, {
      request_id: newRequestId(),
      text,
      ...(mentionedChats.length ? { mentioned_chats: mentionedChats } : {}),
    })
  }

  queueRemove(id, item) {
    return this.call('POST', `/sessions/${seg(id)}/queue/${seg(item)}/remove`, { request_id: newRequestId() })
  }

  // Steer may start a stopped chat first - the boot budget, like resume.
  queueSteer(id, item) {
    return this.call(
      'POST',
      `/sessions/${seg(id)}/queue/${seg(item)}/steer`,
      { request_id: newRequestId() },
      { timeoutMs: BOOT_TIMEOUT_MS },
    )
  }

  queueResume(id) {
    return this.call('POST', `/sessions/${seg(id)}/queue/resume`, { request_id: newRequestId() }, { timeoutMs: BOOT_TIMEOUT_MS })
  }

  // A message straight into the running turn, never through the queue.
  steer(id, text, mentionedChats = []) {
    return this.call(
      'POST',
      `/sessions/${seg(id)}/steer`,
      { request_id: newRequestId(), text, ...(mentionedChats.length ? { mentioned_chats: mentionedChats } : {}) },
      { timeoutMs: BOOT_TIMEOUT_MS },
    )
  }

  // Resuming boots the harness again (the same cold-start window as
  // creating) - the boot budget.
  resume(id) {
    return this.call('POST', `/sessions/${seg(id)}/resume`, { request_id: newRequestId() }, { timeoutMs: BOOT_TIMEOUT_MS })
  }

  decisions(id) {
    return this.call('GET', `/sessions/${seg(id)}/decisions`)
  }

  // ---- machines ----

  machines() {
    return this.call('GET', '/machines')
  }

  createMachine({ name, platform }) {
    return this.call('POST', '/machines', { request_id: newRequestId(), name, platform })
  }

  removeMachine(id) {
    return this.call('DELETE', `/machines/${seg(id)}?request_id=${encodeURIComponent(newRequestId())}`)
  }

  // The long one: an installer runs minutes - the receipt carries the
  // wait. `withRuntime` is the human's go-ahead to set up a runtime the
  // harness needs (pi's Node.js); the daemon never assumes it. No
  // timeout here: the wait is the contract (the daemon holds its own
  // 600 s limit).
  installHarness(machineId, harnessId, { withRuntime = false } = {}) {
    return this.call(
      'POST',
      `/machines/${seg(machineId)}/harnesses`,
      {
        request_id: newRequestId(),
        harness: harnessId,
        with_runtime: withRuntime,
      },
      { timeoutMs: INSTALL_TIMEOUT_MS },
    )
  }

  // ---- plugins (Plugins) ----

  plugins() {
    return this.call('GET', '/plugins')
  }

  // Reading a plugin fetches it first - a clone or a download can take a
  // while - and installs nothing; the answer is what it asks for.
  stagePlugin(source) {
    return this.call('POST', '/plugins/stage', { request_id: newRequestId(), source }, { timeoutMs: 180_000 })
  }

  // Installing takes the staged copy and runs whatever makes the plugin
  // - for a marketplace entry, an install command that can take minutes
  // (the review screen says it runs again each time a Claude chat starts).
  // No client timeout, like the harness installer: the daemon holds its
  // own limit, and a client that gave up first would report a failure
  // while the install finished without anyone to show it.
  installStagedPlugin(stageId) {
    return this.call('POST', `/plugins/stage/${seg(stageId)}/install`, { request_id: newRequestId() }, { timeoutMs: INSTALL_TIMEOUT_MS })
  }

  cancelStagedPlugin(stageId) {
    return this.call('POST', `/plugins/stage/${seg(stageId)}/cancel`, { request_id: newRequestId() })
  }

  // An installed plugin's id is name@marketplace; a built-in's is the
  // harness id.
  setPluginEnabled(id, enabled) {
    return this.call('POST', `/plugins/${seg(id)}/enabled`, { request_id: newRequestId(), enabled })
  }

  // Removing deletes the plugin's whole directory: a large plugin on a
  // slow disk can outlast the normal budget, and a timeout would report a
  // failure for a removal that happened. Bounded rather than exempt,
  // because a daemon that is hung should still give the pane its answer.
  removePlugin(id) {
    return this.call('POST', `/plugins/${seg(id)}/remove`, { request_id: newRequestId() }, { timeoutMs: 180_000 })
  }

  // ---- marketplaces (Plugins > Discover, Marketplaces) ----

  marketplaces() {
    return this.call('GET', '/marketplaces')
  }

  // Every enabled marketplace's entries, from the cached catalogs.
  marketplaceCatalog() {
    return this.call('GET', '/marketplaces/catalog')
  }

  // Adding and refreshing fetch the catalog - a clone can take a while.
  addMarketplace(source) {
    return this.call('POST', '/marketplaces', { request_id: newRequestId(), source }, { timeoutMs: 180_000 })
  }

  refreshMarketplace(name) {
    return this.call(
      'POST',
      `/marketplaces/${seg(name)}/refresh`,
      { request_id: newRequestId() },
      { timeoutMs: 180_000 },
    )
  }

  setMarketplaceEnabled(name, enabled) {
    return this.call('POST', `/marketplaces/${seg(name)}/enabled`, {
      request_id: newRequestId(),
      enabled,
    })
  }

  removeMarketplace(name) {
    return this.call('POST', `/marketplaces/${seg(name)}/remove`, { request_id: newRequestId() })
  }

  // ---- MCP servers (Settings > MCP servers) ----

  mcpServers() {
    return this.call('GET', '/mcp-servers')
  }

  addMcpServer(machineId, { name, detail, command, needsKey }) {
    return this.call('POST', '/mcp-servers', {
      request_id: newRequestId(),
      machine: machineId,
      name,
      detail,
      command,
      needs_key: needsKey,
    })
  }

  removeMcpServer(id) {
    return this.call('DELETE', `/mcp-servers/${seg(id)}?request_id=${encodeURIComponent(newRequestId())}`)
  }

  setMcpServerEnabled(id, enabled) {
    return this.call('POST', `/mcp-servers/${seg(id)}/enabled`, {
      request_id: newRequestId(),
      enabled,
    })
  }

  acknowledgeMcpServerKey(id) {
    return this.call('POST', `/mcp-servers/${seg(id)}/key`, { request_id: newRequestId() })
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
      ...(rule.permission_mode ? { permission_mode: rule.permission_mode } : {}),
      workspace: rule.workspace,
      machine: rule.machine,
      task: rule.task,
      ...(rule.id ? { id: rule.id, enabled: rule.enabled } : {}),
    })
  }

  removeRule(ruleId) {
    return this.call(
      'DELETE',
      `/automations/${seg(ruleId)}?request_id=${encodeURIComponent(newRequestId())}`,
    )
  }

  setRuleEnabled(ruleId, enabled) {
    return this.call('POST', `/automations/${seg(ruleId)}/enabled`, {
      request_id: newRequestId(),
      enabled,
    })
  }

  // Run now creates the chat and delivers the task - a harness boot
  // rides the request, so the boot budget.
  runRule(ruleId) {
    return this.call(
      'POST',
      `/automations/${seg(ruleId)}/run`,
      { request_id: newRequestId() },
      { timeoutMs: BOOT_TIMEOUT_MS },
    )
  }

  // ---- projects ----

  /** The registered projects, daemon-side (the same list every surface reads). */
  projects() {
    return this.call('GET', '/projects')
  }

  /** Register a folder as a project. One folder this release; the array is
   *  the multi-folder future. */
  createProject({ folders }) {
    return this.call('POST', '/projects', {
      request_id: newRequestId(),
      folders,
    })
  }

  /** Unregister - the folder on disk and its chats are untouched. */
  removeProject(projectId) {
    return this.call(
      'DELETE',
      `/projects/${seg(projectId)}?request_id=${encodeURIComponent(newRequestId())}`,
    )
  }

  // The models a harness advertises, in its own words (empty = the slot
  // stays reserved for that harness). The catalog comes from a
  // short-lived harness CLI (a 30 s handshake daemon-side) - the boot
  // budget, so a cold CLI is not cut off mid-handshake.
  models(harnessId) {
    return this.call('GET', `/harnesses/${seg(harnessId)}/models`, null, { timeoutMs: BOOT_TIMEOUT_MS })
  }

  // Capture this harness's catalog again through the plugin's own driver.
  // The daemon refuses with 409 while a chat on the harness is alive and
  // with 503 carrying the driver's sentence when the capture fails; both
  // arrive as error.message. The answer rides the receipt envelope, so the
  // fresh harness row is under `result`. The budget covers the daemon's own
  // 60 s capture deadline plus the handshake around it.
  async catalogRefresh(harnessId) {
    const reply = await this.call(
      'POST',
      `/harnesses/${seg(harnessId)}/catalog/refresh`,
      { request_id: newRequestId() },
      { timeoutMs: 90_000 },
    )
    return reply?.result ?? reply
  }

  // ---- custom model endpoints (machine-local, secret-safe) ----

  // The endpoint records on the machine that answers, metadata only -
  // the secret never leaves the daemon. `harness` narrows to one.
  modelEndpoints({ harness = null } = {}) {
    const query = harness ? `?harness=${encodeURIComponent(harness)}` : ''
    return this.call('GET', `/model-endpoints${query}`)
  }

  // Create or replace one endpoint record. `id` edits; without it a new
  // record is made. `authKind` is one of bearer, api_key, none. The
  // answer is the metadata view - the key is never returned.
  saveEndpoint({ id = null, harness, name, baseUrl, modelsPath = null, defaultModel = null, authKind, key = null, enabled = true, isDefault = false }) {
    return this.call('POST', '/model-endpoints', {
      request_id: newRequestId(),
      ...(id ? { id } : {}),
      harness,
      name,
      base_url: baseUrl,
      ...(modelsPath ? { models_path: modelsPath } : {}),
      ...(defaultModel ? { default_model: defaultModel } : {}),
      auth_kind: authKind,
      ...(key ? { key } : {}),
      enabled,
      is_default: isDefault,
    })
  }

  removeEndpoint(id) {
    return this.call('DELETE', `/model-endpoints/${seg(id)}?request_id=${encodeURIComponent(newRequestId())}`)
  }

  setEndpointEnabled(id, enabled) {
    return this.call('POST', `/model-endpoints/${seg(id)}/enabled`, {
      request_id: newRequestId(),
      enabled,
    })
  }

  // Name a harness's machine default endpoint, or clear it with no id.
  setDefaultEndpoint(harness, id = null) {
    return this.call('POST', '/model-endpoints/default', {
      request_id: newRequestId(),
      harness,
      ...(id ? { id } : {}),
    })
  }

  // The models a custom endpoint lists today, fetched live through its
  // own key. A failing gateway answers as an error, never an empty
  // advertisement the picker could mistake for "none".
  endpointModels(id) {
    return this.call('GET', `/model-endpoints/${seg(id)}/models`, null, { timeoutMs: BOOT_TIMEOUT_MS })
  }

  // Point a stopped chat at another endpoint (or back at the harness's
  // own provider with no id). A live chat answers 409 - stop it first.
  setSessionEndpoint(id, endpointId = null) {
    return this.call('POST', `/sessions/${seg(id)}/endpoint`, {
      request_id: newRequestId(),
      ...(endpointId ? { endpoint_id: endpointId } : {}),
    })
  }

  // ---- harness sign-in ----

  // Start the harness's own login command, relayed: the returned view is
  // the first beat; poll signInView for the CLI's words, feed the human's
  // answers through signInInput.
  startSignIn(harnessId) {
    return this.call('POST', `/harnesses/${seg(harnessId)}/signin`, {
      request_id: newRequestId(),
    })
  }

  signInView(harnessId) {
    return this.call('GET', `/harnesses/${seg(harnessId)}/signin`)
  }

  signInInput(harnessId, text) {
    return this.call('POST', `/harnesses/${seg(harnessId)}/signin/input`, {
      request_id: newRequestId(),
      text,
    })
  }

  // Stop a running sign-in relay - the abandoned-browser-flow answer.
  stopSignIn(harnessId) {
    return this.call('POST', `/harnesses/${seg(harnessId)}/signin/stop`, {
      request_id: newRequestId(),
    })
  }

  answer(decisionId, choice) {
    return this.call('POST', `/decisions/${seg(decisionId)}/answer`, {
      request_id: newRequestId(),
      choice,
    })
  }

  // The live event stream for one session. `onEvent` receives each parsed
  // event; the reader keeps going until the connection drops, then reports
  // the last cursor through `onClose(cursor, err)` so the caller can resume
  // with `after` - or stop retrying when `err` carries the daemon's own
  // "no such session" (see sessionGone). The cursor names the last frame
  // that completed: a stream that dies between a frame's `id:` line and its
  // data must not claim that seq, or the resume steps over an event the
  // console never folded and it is lost for good. Returns a stop function.
  events(id, { after = null, onEvent, onClose, idleMs = STREAM_IDLE_MS }) {
    const controller = new AbortController()
    const run = async () => {
      // A garbage cursor is deliberately absent: one clean full replay
      // rather than a resume that can never find its position.
      let cursor = parseCursor(after)
      let skipped = 0
      // The watchdog. Every chunk restarts it and nothing else does: silence
      // past the deadline is a half-open socket, and waiting on a body that
      // will never speak again is the frozen chat. The wait is raced here
      // rather than only aborted, because only a real fetch ties the
      // response body to the signal - the read itself must end either way.
      let quiet = null
      const readOrQuiet = async (reader) => {
        const deadline = new Promise((_, reject) => {
          quiet = setTimeout(() => {
            const error = new Error('the event stream went quiet')
            error.idle = true
            controller.abort()
            reject(error)
          }, idleMs)
        })
        try {
          return await Promise.race([reader.read(), deadline])
        } finally {
          clearTimeout(quiet)
        }
      }
      let reader = null
      try {
        const query = cursor == null ? '' : `?after=${cursor}`
        const response = await this.fetch(`${this.url}${this.prefix}/sessions/${seg(id)}/events${query}`, {
          headers: { Authorization: `Bearer ${this.token}` },
          // No timeout: this connection's lifetime is the stream itself
          // (the stop handle and the poll decide when it ends).
          signal: controller.signal,
        })
        if (!response.ok || !response.body) {
          // The status rides the error (the same shape as call()): the
          // close callback must be able to tell a dead session from a
          // dead connection, or a deleted chat retries forever.
          const error = new Error(`SSE HTTP ${response.status}`)
          error.status = response.status
          throw error
        }
        reader = response.body.getReader()
        const decoder = new TextDecoder()
        let buffer = ''
        // The frame being read, its lines joined. SSE ends a frame with a
        // blank line, so until that blank line the payload may still be
        // half written - which is why a frame still open when the stream
        // dies is dropped rather than parsed.
        let frame = ''
        const fold = (text) => {
          const parsed = parseSseFrame(text)
          if (!parsed) return
          if (parsed.event) {
            // A read frame ends the run of skips: the next unreadable frame
            // is a new hole and gets its own note.
            skipped = 0
            // The event's own seq when it carries a sane one; the frame's
            // id line otherwise. Either way the cursor only moves
            // forward to a real position.
            cursor = parseCursor(parsed.event.seq) ?? parsed.seq ?? cursor
            onEvent(parsed.event)
            return
          }
          if (parsed.malformed) {
            // A frame the console cannot read is skipped, not fatal: its
            // seq arrived with the frame, so the cursor still moves past
            // it and a reconnect never replays into the same bad line
            // forever.
            skipped += 1
            if (skipped === 1 || skipped === 10) {
              console.error(`OpenRemote: skipped ${skipped} unreadable event frames in session ${id}`)
            }
            // The transcript hears about the hole once per run: the reducer
            // turns this into one note, so the gap is visible instead of
            // silent.
            if (skipped === 1) onEvent({ type: 'stream_gap' })
            cursor = parsed.seq ?? cursor
          }
          // An id with no data behind it is not a frame: the cursor stays.
        }
        while (true) {
          const { done, value } = await readOrQuiet(reader)
          if (done) break
          buffer += decoder.decode(value, { stream: true })
          let index
          while ((index = buffer.indexOf('\n')) >= 0) {
            const line = buffer.slice(0, index).replace(/\r$/, '')
            buffer = buffer.slice(index + 1)
            if (line === '') {
              fold(frame)
              frame = ''
            } else {
              frame += (frame ? '\n' : '') + line
            }
          }
        }
        onClose?.(cursor)
      } catch (err) {
        if (err?.idle) {
          // The watchdog's own close: the caller must see it, or the chat
          // never re-dials and never says anything again.
          reader?.cancel?.().catch(() => {})
          onClose?.(cursor, err)
        } else if (err?.name !== 'AbortError') {
          // Anything that is not this handle's stop reports the last
          // cursor, and the error itself rides along as the second
          // argument so the caller can stop retrying when the session is
          // gone.
          onClose?.(cursor, err)
        }
      } finally {
        clearTimeout(quiet)
      }
    }
    run()
    return () => controller.abort()
  }
}

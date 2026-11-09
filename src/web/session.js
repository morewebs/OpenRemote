// What the web console remembers between page loads: the account's tokens,
// the pending sign-in, and which machine the person picked.
//
// Browser storage rather than the daemon's own files, because there is no
// daemon on this machine: the browser is the device. Every read is guarded,
// since a profile with storage disabled must still be able to sign in for this
// visit.

const TOKENS = 'openremote-web-tokens'
const PENDING = 'openremote-web-signin'
const DEVICE = 'openremote-web-device'

function read(key) {
  try {
    const raw = localStorage.getItem(key)
    return raw ? JSON.parse(raw) : null
  } catch {
    return null
  }
}

function write(key, value) {
  try {
    if (value == null) localStorage.removeItem(key)
    else localStorage.setItem(key, JSON.stringify(value))
    return true
  } catch {
    return false
  }
}

/// `{ access, refresh, expiresAt }` from the last sign-in, or null.
export function loadTokens() {
  const saved = read(TOKENS)
  if (!saved || typeof saved.access !== 'string' || !saved.access) return null
  return {
    access: saved.access,
    refresh: typeof saved.refresh === 'string' ? saved.refresh : null,
    expiresAt: Number.isFinite(saved.expiresAt) ? saved.expiresAt : 0,
  }
}

export function saveTokens(tokens) {
  return write(TOKENS, tokens)
}

export function clearTokens() {
  return write(TOKENS, null)
}

// A sign-in that is older than this cannot still be in flight: the gateway's
// authorization code lives minutes, and a stale verifier is not a secret worth
// carrying for ever.
const PENDING_MS = 10 * 60_000

/// The pending sign-ins, keyed by their own `state`. The old build stored one
/// object in this slot; it is read as a one-entry map so a callback already in
/// flight across the deploy still finishes.
function pendingEntries() {
  const saved = read(PENDING)
  if (!saved || typeof saved !== 'object') return {}
  if (typeof saved.state === 'string' && typeof saved.verifier === 'string') {
    return { [saved.state]: saved }
  }
  const entries = {}
  for (const [state, entry] of Object.entries(saved)) {
    if (entry && typeof entry === 'object' && typeof entry.verifier === 'string') {
      entries[state] = entry
    }
  }
  return entries
}

function pendingLive(entry) {
  const at = Number(entry?.savedAt)
  // An entry written by the old build carries no stamp; a deploy must not
  // strand a sign-in that is already in the gateway's hands.
  return !Number.isFinite(at) || Date.now() - at <= PENDING_MS
}

/**
 * The PKCE verifier and state of the sign-in whose `state` the gateway sent
 * back. Every sign-in keeps its own entry, so a second tab or a second click
 * cannot hand the first callback somebody else's verifier.
 *
 * It also carries the gateway the sign-in started at. The return address the
 * gateway sends the browser to holds only the code and the state, so a page
 * pointed at another backend by `?api=` or `window.__OPENREMOTE_CLOUD__` would
 * otherwise finish the exchange against the default one.
 */
export function loadPending(state) {
  if (typeof state !== 'string' || !state) return null
  const entry = pendingEntries()[state]
  if (!entry || typeof entry.verifier !== 'string' || !pendingLive(entry)) return null
  return {
    verifier: entry.verifier,
    state: entry.state ?? state,
    redirect: String(entry.redirect ?? ''),
    api: String(entry.api ?? ''),
    issuer: String(entry.issuer ?? ''),
  }
}

export function savePending(pending) {
  const state = pending?.state
  if (typeof state !== 'string' || !state) return false
  const entries = pendingEntries()
  // Drop what can no longer finish, so one slot cannot ratchet with every
  // attempt a person ever made.
  for (const [key, entry] of Object.entries(entries)) {
    if (!pendingLive(entry)) delete entries[key]
  }
  entries[state] = { ...pending, savedAt: Date.now() }
  return write(PENDING, entries)
}

/// Remove one sign-in by its state, or every one when no state is named
/// (sign-out, or a gateway refusal that names no state).
export function clearPending(state) {
  if (typeof state !== 'string' || !state) return write(PENDING, null)
  const entries = pendingEntries()
  delete entries[state]
  return write(PENDING, Object.keys(entries).length ? entries : null)
}

/// The machine the person picked, with the name to show while it connects.
export function loadDevice() {
  const saved = read(DEVICE)
  if (!saved || typeof saved.id !== 'string' || !saved.id) return null
  return { id: saved.id, name: typeof saved.name === 'string' ? saved.name : '' }
}

export function saveDevice(device) {
  return write(DEVICE, device)
}

export function clearDevice() {
  return write(DEVICE, null)
}

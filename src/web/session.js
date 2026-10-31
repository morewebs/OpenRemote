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

/// The PKCE verifier and state of a sign-in that is in flight, so a page that
/// came back from the gateway can finish the exchange it started.
///
/// It also carries the gateway the sign-in started at. The return address the
/// gateway sends the browser to holds only the code and the state, so a page
/// pointed at another backend by `?api=` or `window.__OPENREMOTE_CLOUD__` would
/// otherwise finish the exchange against the default one.
export function loadPending() {
  const saved = read(PENDING)
  if (!saved || typeof saved.verifier !== 'string' || typeof saved.state !== 'string') return null
  return {
    verifier: saved.verifier,
    state: saved.state,
    redirect: String(saved.redirect ?? ''),
    api: String(saved.api ?? ''),
    issuer: String(saved.issuer ?? ''),
  }
}

export function savePending(pending) {
  return write(PENDING, pending)
}

export function clearPending() {
  return write(PENDING, null)
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

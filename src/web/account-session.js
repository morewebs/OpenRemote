import { CLIENT_ID, tokenUrl } from './cloud.js'
import { clearTokens, loadTokens, saveTokens } from './session.js'

function expiredSession() {
  return Object.assign(new Error('Your sign-in has ended. Sign in with moreweb again.'), { code: 'session_expired' })
}

// Account requests share renewal. Web Locks also serialize tabs, because a
// rotated refresh token must only be spent once across the whole origin.
export function createAccountSession({
  fetch = (...args) => globalThis.fetch(...args),
  load = loadTokens, save = saveTokens, clear = clearTokens,
  now = Date.now, locks = globalThis.navigator?.locks,
} = {}) {
  const refreshing = new Map()
  const same = (a, b) => a?.access === b?.access && a?.refresh === b?.refresh

  async function renew(target, rejectedAccess) {
    const previous = load()
    if (!previous) throw expiredSession()
    if (rejectedAccess && previous.access !== rejectedAccess) return previous
    if (!rejectedAccess && (!previous.expiresAt || previous.expiresAt > now() + 60000)) return previous
    if (!previous.refresh) {
      // Legacy sessions without a refresh token may still have valid access.
      if (!rejectedAccess) return previous
      throw expiredSession()
    }
    const currentSession = () => {
      const current = load()
      if (!current) throw expiredSession()
      return current
    }
    try {
      const response = await fetch(tokenUrl(target.issuer), {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({ grant_type: 'refresh_token', refresh_token: previous.refresh, client_id: CLIENT_ID }).toString(),
        signal: AbortSignal.timeout(15000),
      })
      const payload = await response.json().catch(() => null)
      // A logout or another sign-in during the request owns the newer state.
      const current = currentSession()
      if (!same(current, previous)) return current
      if (!response.ok) {
        if ((response.status === 400 || response.status === 401) && payload?.error === 'invalid_grant') {
          clear()
          throw expiredSession()
        }
        throw new Error('Your sign-in could not be renewed right now. Try again when the connection is available.')
      }
      if (typeof payload?.access_token !== 'string' || !payload.access_token ||
          !Number.isFinite(payload.expires_in) || payload.expires_in <= 0) {
        throw new Error('The sign-in service returned an incomplete renewal. Please try again.')
      }
      const next = {
        ...previous,
        access: payload.access_token,
        refresh: typeof payload.refresh_token === 'string' && payload.refresh_token ? payload.refresh_token : previous.refresh,
        expiresAt: now() + payload.expires_in * 1000,
      }
      save(next)
      return next
    } catch (error) {
      const current = load()
      if (current && !same(current, previous)) return current
      throw error
    }
  }

  async function tokens(target, rejectedAccess) {
    const key = target.issuer
    if (refreshing.has(key)) return refreshing.get(key)
    const work = locks?.request
      ? locks.request('openremote-web-token-refresh', () => renew(target, rejectedAccess))
      : renew(target, rejectedAccess)
    refreshing.set(key, work)
    try { return await work } finally { refreshing.delete(key) }
  }

  async function request(target, path) {
    let account = await tokens(target)
    const send = () => fetch(`${target.api}${path}`, { headers: { Authorization: `Bearer ${account.access}` } })
    let response = await send()
    if (response.status === 401) {
      account = await tokens(target, account.access)
      response = await send()
    }
    return response
  }
  return { tokens, request }
}

const accountSession = createAccountSession()
export const accountTokens = accountSession.tokens
export const accountFetch = accountSession.request

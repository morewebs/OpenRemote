// Where the web console signs in, and the URLs that sign-in uses.
//
// Pure, and the one place these values live: the page may point a test build
// at another gateway, and a person reading this file should not have to hunt
// for a second copy of the issuer. Nothing here is consulted by the peer
// module: the encrypted link takes request paths and never an origin, so a
// value a page supplies can change where the console signs in and can never
// change where a sealed request goes.

/// The moreweb gateway and OAuth issuer the shipped build uses.
export const PRODUCTION = {
  api: 'https://api.moreweb.space/v1/openremote',
  issuer: 'https://auth.moreweb.space/v1/auth',
}

/// The public OAuth client the desktop app already uses. A browser needs a
/// public client with PKCE, and this one is registered for that.
export const CLIENT_ID = 'openremote-desktop'

/// Where the shipped web console lives, and where the browser comes back.
export const APP_ORIGIN = 'https://openremote.moreweb.space'
export const APP_REDIRECT = `${APP_ORIGIN}/app/`

/// Whether an address is https, or http on this machine. A page may only
/// narrow the gateway to a secure address (or to a loopback one for a local
/// test), never to some plaintext host it names.
function usable(url) {
  if (typeof url !== 'string') return null
  const text = url.trim().replace(/\/+$/, '')
  if (!text) return null
  if (/^https:\/\/\S+$/i.test(text)) return text
  if (/^http:\/\/(127(?:\.\d{1,3}){3}|localhost|\[::1\])(:\d{1,5})?(\/\S*)?$/i.test(text)) return text
  return null
}

/**
 * The gateway this page talks to.
 *
 * The page's own configuration wins, because a local check of the console runs
 * against the project's in-memory gateway and the console should follow the
 * page it was served from; `?api=` and `?issuer=` let a build point at one
 * without editing anything. Otherwise the production values stand, so a page
 * that sets nothing gets the real service and an unusable override is ignored
 * rather than half-applied.
 *
 * `env` is `window.__OPENREMOTE_CLOUD__` when the page set it; `search` is the
 * page's query string. Both are passed in so this can be tested without a
 * browser.
 */
export function cloudTarget({ search = '', env = null } = {}) {
  const query = new URLSearchParams(search)
  const fromQuery = (key) => usable(query.get(key))
  const fromPage = (key) => usable(env && typeof env === 'object' ? env[key] : null)
  return {
    api: fromQuery('api') ?? fromPage('api') ?? PRODUCTION.api,
    issuer: fromQuery('issuer') ?? fromPage('issuer') ?? PRODUCTION.issuer,
  }
}

/**
 * The redirect this page's sign-in uses.
 *
 * The gateway only accepts a redirect it has registered, so the two the console
 * is registered for are the production page and a loopback dev server on the
 * port it is actually served from. Anything else (a LAN address, an unexpected
 * origin) is returned as that origin itself so a misconfigured build fails at
 * the gateway instead of silently signing in somewhere else.
 */
export function redirectFor({ protocol = '', hostname = '', port = '', origin = '' } = {}) {
  const loopback = hostname === '127.0.0.1' || hostname === 'localhost' || hostname === '[::1]' || hostname === '::1'
  if (loopback) {
    // The dev and preview servers serve this page under the same `/app/` path
    // the shipped one lives at, so a loopback sign-in lands on the console
    // rather than on the server's root, which has nothing there.
    const host = hostname === 'localhost' ? 'localhost' : hostname
    return `http://${host}${port ? `:${port}` : ''}/app/`
  }
  if (`${protocol}//${hostname}` === APP_ORIGIN) return APP_REDIRECT
  return origin ? `${origin.replace(/\/+$/, '')}/` : APP_REDIRECT
}

/** The authorize page for one sign-in: authorization code with PKCE, S256. */
export function authorizeUrl({ issuer, redirect, state, challenge }) {
  const query = new URLSearchParams({
    response_type: 'code',
    client_id: CLIENT_ID,
    redirect_uri: redirect,
    scope: 'openid profile email',
    state,
    code_challenge: challenge,
    code_challenge_method: 'S256',
  })
  return `${issuer.replace(/\/+$/, '')}/oauth/authorize?${query.toString()}`
}

/** The token endpoint a browser exchanges its code at. */
export function tokenUrl(issuer) {
  return `${issuer.replace(/\/+$/, '')}/oauth/token`
}

/** The form body of that exchange, as the gateway reads it. */
export function tokenBody({ code, redirect, verifier }) {
  return new URLSearchParams({
    grant_type: 'authorization_code',
    code,
    redirect_uri: redirect,
    client_id: CLIENT_ID,
    code_verifier: verifier,
  }).toString()
}

/** Base64url without padding, the encoding PKCE uses. */
export function base64url(bytes) {
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

/** A PKCE verifier: 32 random bytes, base64url. */
export function verifierFor(crypto = globalThis.crypto) {
  const bytes = new Uint8Array(32)
  crypto.getRandomValues(bytes)
  return base64url(bytes)
}

/** The S256 challenge for a verifier. */
export async function challengeFor(verifier, crypto = globalThis.crypto) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier))
  return base64url(new Uint8Array(digest))
}

/** A fresh `state` for one sign-in: 16 random bytes, base64url. */
export function stateFor(crypto = globalThis.crypto) {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return base64url(bytes)
}

// The web console's sign-in URLs and its one configuration hook.
//
// What is pinned here is what a reviewer cannot see by reading a component:
// that a page which sets nothing gets production, that a page can point a
// local check at the in-memory gateway without editing source, that an address
// a page supplies can never be plaintext or junk, and that the authorize URL
// is the exact PKCE shape the gateway expects.

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  APP_REDIRECT,
  CLIENT_ID,
  PRODUCTION,
  authorizeUrl,
  challengeFor,
  cloudTarget,
  redirectFor,
  tokenBody,
  tokenUrl,
  verifierFor,
} from './cloud.js'

test('a page that sets nothing talks to production', () => {
  const target = cloudTarget({ search: '' })
  assert.deepEqual(target, PRODUCTION)
  assert.equal(target.api, 'https://api.moreweb.space/v1/openremote')
  assert.equal(target.issuer, 'https://auth.moreweb.space/v1/auth')
  assert.equal(cloudTarget({}).api, PRODUCTION.api)
  assert.equal(cloudTarget({ search: '?api=', env: null }).api, PRODUCTION.api, 'an empty override is not one')
})

test('a query or a page object can point the console at another gateway', () => {
  const query = cloudTarget({ search: '?api=http://127.0.0.1:8099/v1/openremote&issuer=http://127.0.0.1:8099/v1/auth' })
  assert.equal(query.api, 'http://127.0.0.1:8099/v1/openremote', 'a loopback test gateway is allowed over http')
  assert.equal(query.issuer, 'http://127.0.0.1:8099/v1/auth')
  const page = cloudTarget({ env: { api: 'https://staging.example/v1/openremote' } })
  assert.equal(page.api, 'https://staging.example/v1/openremote')
  assert.equal(page.issuer, PRODUCTION.issuer, 'one field does not drag the other with it')
  // The query wins over the page object: it is the more specific instruction.
  const both = cloudTarget({ search: '?api=https://query.example/v1/openremote', env: { api: 'https://page.example/v1/openremote' } })
  assert.equal(both.api, 'https://query.example/v1/openremote')
})

test('an address a page supplies can never be plaintext or junk', () => {
  for (const given of ['http://example.com/v1/openremote', 'javascript:alert(1)', 'file:///etc/passwd', 'not a url', '//evil.example']) {
    const target = cloudTarget({ search: `?api=${encodeURIComponent(given)}&issuer=${encodeURIComponent(given)}` })
    assert.equal(target.api, PRODUCTION.api, `${given} is ignored`)
    assert.equal(target.issuer, PRODUCTION.issuer, `${given} is ignored for the issuer too`)
  }
})

test('the redirect is the registered page, or the loopback port it is served from', () => {
  assert.equal(redirectFor({ protocol: 'https:', hostname: 'openremote.moreweb.space', origin: 'https://openremote.moreweb.space' }), APP_REDIRECT)
  assert.equal(APP_REDIRECT, 'https://openremote.moreweb.space/app/')
  // A loopback server serves this page under the same `/app/` path the shipped
  // one lives at, so the sign-in comes back to the console and not to the
  // server's root, which has nothing there.
  assert.equal(
    redirectFor({ protocol: 'http:', hostname: '127.0.0.1', port: '5177', origin: 'http://127.0.0.1:5177' }),
    'http://127.0.0.1:5177/app/',
    'a local check signs in back to the page it is served from',
  )
  assert.equal(redirectFor({ protocol: 'http:', hostname: 'localhost', port: '5176', origin: 'http://localhost:5176' }), 'http://localhost:5176/app/')
  // An origin nobody registered is returned as itself, so the gateway refuses
  // it instead of the page silently signing in somewhere else.
  assert.equal(redirectFor({ protocol: 'https:', hostname: 'dev.example', origin: 'https://dev.example' }), 'https://dev.example/')
  assert.equal(redirectFor({}), APP_REDIRECT)
})

test('the authorize URL is the PKCE shape the gateway reads', () => {
  const url = new URL(
    authorizeUrl({
      issuer: 'https://auth.moreweb.space/v1/auth',
      redirect: APP_REDIRECT,
      state: 'state-123',
      challenge: 'challenge-456',
    }),
  )
  assert.equal(`${url.origin}${url.pathname}`, 'https://auth.moreweb.space/v1/auth/oauth/authorize')
  assert.equal(url.searchParams.get('response_type'), 'code')
  assert.equal(url.searchParams.get('client_id'), CLIENT_ID)
  assert.equal(CLIENT_ID, 'openremote-desktop')
  assert.equal(url.searchParams.get('redirect_uri'), APP_REDIRECT)
  assert.equal(url.searchParams.get('state'), 'state-123')
  assert.equal(url.searchParams.get('code_challenge'), 'challenge-456')
  assert.equal(url.searchParams.get('code_challenge_method'), 'S256')
  assert.equal(url.searchParams.get('scope'), 'openid profile email')
  assert.equal(tokenUrl('https://auth.moreweb.space/v1/auth/'), 'https://auth.moreweb.space/v1/auth/oauth/token')
})

test('the token exchange carries the verifier and the same redirect', () => {
  const body = new URLSearchParams(tokenBody({ code: 'abc', redirect: APP_REDIRECT, verifier: 'ver' }))
  assert.equal(body.get('grant_type'), 'authorization_code')
  assert.equal(body.get('code'), 'abc')
  assert.equal(body.get('code_verifier'), 'ver')
  assert.equal(body.get('client_id'), CLIENT_ID)
  assert.equal(body.get('redirect_uri'), APP_REDIRECT)
})

test('the verifier and challenge are the RFC 7636 pair', async () => {
  // The RFC's own vector: the challenge is base64url(SHA-256(verifier)).
  const challenge = await challengeFor('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk')
  assert.equal(challenge, 'E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM')
  const verifier = verifierFor()
  assert.match(verifier, /^[A-Za-z0-9_-]{43}$/, '32 random bytes, base64url and no padding')
})

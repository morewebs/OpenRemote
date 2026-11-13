import assert from 'node:assert/strict'
import test from 'node:test'
import { createAccountSession } from './account-session.js'

const target = { api: 'https://api.example.test/v1/openremote', issuer: 'https://auth.example.test/v1/auth' }
const expired = { access: 'old', refresh: 'refresh-old', expiresAt: 1000 }
function setup(fetch, initial = expired) {
  let saved = initial
  const session = createAccountSession({
    fetch, now: () => 100000,
    load: () => saved, save: (value) => { saved = value }, clear: () => { saved = null },
    locks: null,
  })
  return { ...session, saved: () => saved, replace: (value) => { saved = value } }
}
const renewed = () => Response.json({ access_token: 'new', refresh_token: 'refresh-new', expires_in: 3600 })

test('an expired access token renews and persists the rotated refresh token', async () => {
  const session = setup(async (url, init) => {
    assert.equal(url, 'https://auth.example.test/v1/auth/oauth/token')
    const body = new URLSearchParams(init.body)
    assert.equal(body.get('grant_type'), 'refresh_token')
    assert.equal(body.get('refresh_token'), 'refresh-old')
    assert.equal(body.get('client_id'), 'openremote-desktop')
    return renewed()
  })
  assert.equal((await session.tokens(target)).access, 'new')
  assert.deepEqual(session.saved(), { access: 'new', refresh: 'refresh-new', expiresAt: 3700000 })
})

test('temporary refresh failures retain credentials for another attempt', async () => {
  for (const failure of [() => { throw new TypeError('offline') }, () => new Response('', { status: 503 }), () => Response.json({})]) {
    const session = setup(async () => failure())
    await assert.rejects(session.tokens(target))
    assert.deepEqual(session.saved(), expired)
  }
})

test('only a definitive invalid refresh grant clears the saved session', async () => {
  const session = setup(async () => Response.json({ error: 'invalid_grant' }, { status: 400 }))
  await assert.rejects(session.tokens(target), { code: 'session_expired' })
  assert.equal(session.saved(), null)
})

test('a 401 renews even an apparently fresh token and retries the account request once', async () => {
  const calls = []
  const session = setup(async (url, init) => {
    calls.push([url, new Headers(init.headers).get('Authorization')])
    if (url.endsWith('/oauth/token')) return renewed()
    return init.headers.Authorization === 'Bearer old' ? new Response('', { status: 401 }) : Response.json({ devices: [] })
  }, { ...expired, expiresAt: 999999 })
  assert.equal((await session.request(target, '/devices')).status, 200)
  assert.deepEqual(calls.map((call) => call[1]), ['Bearer old', null, 'Bearer new'])
})

test('concurrent requests share one refresh instead of reusing a rotating token', async () => {
  let calls = 0
  const session = setup(async () => { calls++; await new Promise((resolve) => setTimeout(resolve, 5)); return renewed() })
  const results = await Promise.all([session.tokens(target), session.tokens(target), session.tokens(target)])
  assert.equal(calls, 1)
  assert.deepEqual(results.map((value) => value.access), ['new', 'new', 'new'])
})

test('a refresh finishing after sign-out cannot restore the account', async () => {
  let finish
  const session = setup(() => new Promise((resolve) => { finish = resolve }))
  const pending = session.tokens(target)
  await Promise.resolve()
  session.replace(null)
  finish(renewed())
  await assert.rejects(pending, { code: 'session_expired' })
  assert.equal(session.saved(), null)
})

test('a refresh failure cannot erase a newer sign-in from another tab', async () => {
  const replacement = { access: 'other', refresh: 'other-refresh', expiresAt: 999999 }
  const session = setup(async () => { session.replace(replacement); return Response.json({ error: 'invalid_grant' }, { status: 400 }) })
  assert.equal((await session.tokens(target)).access, 'other')
  assert.deepEqual(session.saved(), replacement)
})

test('a refresh response without a rotated token keeps the current refresh token', async () => {
  const session = setup(async () => Response.json({ access_token: 'new', expires_in: 3600 }))
  assert.equal((await session.tokens(target)).refresh, 'refresh-old')
})

test('a second 401 is returned without looping or discarding the renewable session', async () => {
  let calls = 0
  const session = setup(async (url) => { calls++; return url.endsWith('/oauth/token') ? renewed() : new Response('', { status: 401 }) }, { ...expired, expiresAt: 999999 })
  assert.equal((await session.request(target, '/devices')).status, 401)
  assert.equal(calls, 3)
  assert.equal(session.saved().access, 'new')
})

test('separate tabs reread rotated credentials after acquiring the origin lock', async () => {
  let saved = expired
  let tail = Promise.resolve()
  const locks = { request: (_name, run) => {
    const work = tail.then(run)
    tail = work.catch(() => {})
    return work
  } }
  let calls = 0
  const options = {
    locks, load: () => saved, save: (value) => { saved = value }, clear: () => { saved = null }, now: () => 100000,
    fetch: async () => { calls++; return renewed() },
  }
  const first = createAccountSession(options)
  const second = createAccountSession(options)
  const result = await Promise.all([first.tokens(target), second.tokens(target)])
  assert.equal(calls, 1)
  assert.deepEqual(result.map((value) => value.access), ['new', 'new'])
})

test('fresh access is reused while a token approaching expiry is renewed', async () => {
  let calls = 0
  const session = setup(async () => { calls++; return renewed() }, { ...expired, expiresAt: 200000 })
  assert.equal((await session.tokens(target)).access, 'old')
  assert.equal(calls, 0)
  session.replace({ ...expired, expiresAt: 120000 })
  assert.equal((await session.tokens(target)).access, 'new')
  assert.equal(calls, 1)
})

test('an OAuth-looking server failure does not end the session', async () => {
  const session = setup(async () => Response.json({ error: 'invalid_grant' }, { status: 503 }))
  await assert.rejects(session.tokens(target))
  assert.deepEqual(session.saved(), expired)
})

test('a signed-out session never sends account requests', async () => {
  const session = setup(async () => assert.fail('must not send a request'), null)
  await assert.rejects(session.request(target, '/devices'), { code: 'session_expired' })
})

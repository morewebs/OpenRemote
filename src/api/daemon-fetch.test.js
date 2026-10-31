// The client's one network seam: a DaemonApi built with its own fetch must
// never touch the global one. That is what lets the same console run in a
// browser over an end-to-end encrypted link, where nothing can dial
// 127.0.0.1, while the desktop keeps using the global exactly as before.

import assert from 'node:assert/strict'
import test from 'node:test'
import { DaemonApi } from './daemon.js'

const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })

test('a client built with a stub fetch never calls the global one', async () => {
  const calls = []
  const stub = async (url, init) => {
    calls.push({ url, init })
    return json({ ok: true })
  }
  const real = globalThis.fetch
  let globalCalls = 0
  globalThis.fetch = () => {
    globalCalls += 1
    throw new Error('the global fetch must not be reached')
  }
  try {
    const api = new DaemonApi('http://127.0.0.1:9/', 'secret', '', stub)
    assert.equal(api.fetch, stub, 'the client keeps the fetch it was given')
    assert.deepEqual(await api.health(), { ok: true })
    assert.equal(calls.length, 1, 'the request went through the stub')
    assert.equal(globalCalls, 0, 'and never through the global')
    assert.equal(calls[0].url, 'http://127.0.0.1:9/healthz')
    assert.equal(calls[0].init.headers.Authorization, 'Bearer secret')
    // The device() view is the same client aimed elsewhere, so it carries
    // the same seam instead of falling back to the global.
    const remote = api.device('dev-2')
    await remote.sessions()
    assert.equal(calls.length, 2)
    assert.equal(calls[1].url, 'http://127.0.0.1:9/devices/dev-2/sessions')
    assert.equal(globalCalls, 0)
  } finally {
    globalThis.fetch = real
  }
})

test('a client built without one uses the global fetch', async () => {
  const real = globalThis.fetch
  const seen = []
  globalThis.fetch = async (url) => {
    seen.push(url)
    return json({})
  }
  try {
    const api = new DaemonApi('127.0.0.1:9', 't')
    await api.health()
    assert.deepEqual(seen, ['http://127.0.0.1:9/healthz'])
    assert.equal(api.ready, true)
  } finally {
    globalThis.fetch = real
  }
})

test('the live event stream goes through the same seam', async () => {
  // The stream is the other network path in the client, and the one a link
  // carries differently. A stub that answers a readable body is enough to
  // prove the reader asks the injected fetch rather than the global.
  let touched = 0
  const stub = async () => {
    touched += 1
    const body = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('id: 3\ndata: {"seq":3,"type":"message"}\n\n'))
        controller.close()
      },
    })
    return new Response(body, { status: 200 })
  }
  const real = globalThis.fetch
  globalThis.fetch = () => {
    throw new Error('the global fetch must not be reached')
  }
  try {
    const api = new DaemonApi('http://127.0.0.1:9', 't', '', stub)
    const events = []
    const closed = new Promise((resolve) => {
      api.events('s1', { after: null, onEvent: (e) => events.push(e), onClose: (cursor) => resolve(cursor) })
    })
    const cursor = await closed
    assert.equal(touched, 1, 'the stream used the stub')
    assert.equal(events.length, 1)
    assert.equal(events[0].seq, 3)
    assert.equal(cursor, 3)
  } finally {
    globalThis.fetch = real
  }
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { createServer } from 'node:net'
import {
  DaemonApi,
  KEEP_ALIVE_MS,
  STREAM_IDLE_MS,
  linkAbort,
  nextRetryDelay,
  normaliseUrl,
  parseCursor,
  parseSseFrame,
  sameDaemon,
  sessionGone,
} from './daemon.js'

// ---- the resume cursor ----

test('a cursor counts only as a non-negative integer', () => {
  assert.equal(parseCursor(0), 0)
  assert.equal(parseCursor(41), 41)
  assert.equal(parseCursor('41'), 41)
  assert.equal(parseCursor(null), null)
  assert.equal(parseCursor(undefined), null)
  assert.equal(parseCursor(''), null)
  assert.equal(parseCursor('41x'), null)
  assert.equal(parseCursor(' 41'), null)
  assert.equal(parseCursor('-1'), null)
  assert.equal(parseCursor(-1), null)
  assert.equal(parseCursor(4.5), null)
  assert.equal(parseCursor(NaN), null)
})

// ---- one SSE frame ----

test('a frame carries its seq on an id line and its event on data lines', () => {
  assert.deepEqual(parseSseFrame('id: 7'), { seq: 7 })
  assert.deepEqual(parseSseFrame('id: 0'), { seq: 0 })
  assert.deepEqual(parseSseFrame('data: {"seq":7,"type":"a"}'), {
    seq: null,
    event: { seq: 7, type: 'a' },
  })
  assert.deepEqual(parseSseFrame('data: {not json'), { seq: null, malformed: true })
  // Multi-line data is one JSON value the protocol splits across lines, and
  // the space after the colon is optional. A sender that does either is not
  // a sender the console may ignore.
  assert.deepEqual(parseSseFrame('id: 3\ndata: {"seq":3,\ndata: "type":"split"}'), {
    seq: 3,
    event: { seq: 3, type: 'split' },
  })
  assert.deepEqual(parseSseFrame('id:3\ndata:{"seq":3,"type":"tight"}'), {
    seq: 3,
    event: { seq: 3, type: 'tight' },
  })
  // A carriage return is part of the line ending, not the payload.
  assert.deepEqual(parseSseFrame('id: 4\r\ndata: {"seq":4,"type":"crlf"}\r'), {
    seq: 4,
    event: { seq: 4, type: 'crlf' },
  })
  // Separators, keep-alive comments, and an id that is not a number are
  // nothing the reader can use.
  assert.equal(parseSseFrame(''), null)
  assert.equal(parseSseFrame(': keep-alive'), null)
  assert.equal(parseSseFrame('id: soon'), null)
})

// ---- the SSE reader over a stubbed fetch ----

// Drive `events()` with the daemon's SSE bytes as one finished response;
// the returned promise settles when the reader reports its close. A
// numeric `fail` answers that HTTP status instead of streaming, so the
// close path an error status takes is observable too.
async function collect(frames, { after, fail } = {}) {
  const api = new DaemonApi('http://daemon', 'token')
  const seen = []
  const noted = []
  let url = null
  const realFetch = globalThis.fetch
  const realError = console.error
  globalThis.fetch = async (requestUrl) => {
    url = requestUrl
    if (fail != null) return new Response('no such session', { status: fail })
    const body = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(frames))
        controller.close()
      },
    })
    return new Response(body, { status: 200 })
  }
  console.error = (...args) => noted.push(args.join(' '))
  try {
    const closedWith = await new Promise((resolve) => {
      api.events('s1', {
        after,
        onEvent: (event) => seen.push(event),
        onClose: (cursor, err) => resolve({ cursor, err }),
      })
    })
    return { seen, noted, url, ...closedWith }
  } finally {
    globalThis.fetch = realFetch
    console.error = realError
  }
}

test('well-formed frames reach onEvent and the close reports the last seq', async () => {
  const { seen, cursor, err } = await collect(
    'id: 1\ndata: {"seq":1,"type":"a"}\n\nid: 2\ndata: {"seq":2,"type":"b"}\n\n',
  )
  assert.deepEqual(seen, [
    { seq: 1, type: 'a' },
    { seq: 2, type: 'b' },
  ])
  assert.equal(cursor, 2)
  // A clean end to the stream is not an error.
  assert.equal(err, undefined)
})

test('a malformed frame is skipped, not fatal: reading continues past it', async () => {
  const { seen, cursor, noted } = await collect(
    'id: 1\ndata: {"seq":1,"type":"a"}\n\n' +
      'id: 2\ndata: {not json\n\n' +
      'id: 3\ndata: {"seq":3,"type":"c"}\n\n',
  )
  // The skipped frame is one gap the transcript is told about, between the
  // frames it could read.
  assert.deepEqual(seen.map((event) => event.type), ['a', 'stream_gap', 'c'])
  assert.equal(cursor, 3)
  assert.equal(noted.length, 1)
})

test('a malformed frame that ends the stream still advances the cursor past it', async () => {
  const { seen, cursor } = await collect(
    'id: 1\ndata: {"seq":1,"type":"a"}\n\nid: 2\ndata: torn\n\n',
  )
  assert.deepEqual(seen.map((event) => event.type), ['a', 'stream_gap'])
  // Resuming from 2 starts after the bad frame, never inside it again.
  assert.equal(cursor, 2)
})

test('a stream that dies between a frame id and its data does not claim that seq', async () => {
  // The daemon writes the id first, so a connection that drops right after
  // it hands the reader a seq with no event behind it. Reporting that seq
  // would resume past an event the console never folded, and nothing would
  // replay it.
  const { seen, cursor } = await collect('id: 1\ndata: {"seq":1,"type":"a"}\n\nid: 2\n')
  assert.deepEqual(seen.map((event) => event.type), ['a'])
  assert.equal(cursor, 1)
})

test('a multi-line frame reaches onEvent whole, through a real stream', async () => {
  // The fixture is the one a hand-written sender produces: data split over
  // two lines, CRLF line endings, and one frame with no space after the
  // colon. A reader that took a frame to be one `data: ` line folded
  // nothing here and left the chat empty.
  const { seen, cursor } = await collect(
    'id: 1\r\ndata: {"seq":1,\r\ndata: "type":"split"}\r\n\r\n' +
      'id:2\ndata:{"seq":2,"type":"tight"}\n\n',
  )
  assert.deepEqual(seen, [
    { seq: 1, type: 'split' },
    { seq: 2, type: 'tight' },
  ])
  assert.equal(cursor, 2)
})

test('a frame still open when the stream dies is not folded', async () => {
  // A blank line is what ends a frame, so the last one may be half written.
  // Dropping it and resuming after the last complete frame replays it whole.
  const { seen, cursor } = await collect(
    'id: 1\ndata: {"seq":1,"type":"a"}\n\nid: 2\ndata: {"seq":2,"type":"b"}',
  )
  assert.deepEqual(seen.map((event) => event.type), ['a'])
  assert.equal(cursor, 1)
})

test('a run of malformed frames is noted on the first and again at the tenth', async () => {
  let frames = ''
  for (let seq = 1; seq <= 10; seq += 1) frames += `id: ${seq}\ndata: {oops\n\n`
  const { seen, noted } = await collect(frames)
  // The transcript hears about the hole once per run of skips, not once per
  // frame: a chat with ten bad frames in a row has one gap, not ten.
  assert.deepEqual(seen, [{ type: 'stream_gap' }])
  assert.equal(noted.length, 2)
})

test('a garbage stored cursor is a deliberate full replay: no after param', async () => {
  const { url } = await collect('id: 1\ndata: {"seq":1,"type":"a"}\n\n', { after: '12x' })
  assert.ok(!url.includes('after='))
})

test('a valid stored cursor rides the url', async () => {
  const { url } = await collect('id: 10\ndata: {"seq":10,"type":"a"}\n\n', { after: '9' })
  assert.ok(url.includes('after=9'))
})

// ---- a stream that stops talking ----

// A body the test drives byte by byte: `step` gets the controller and
// decides what arrives, and whether the stream ever closes.
function stubStream(step) {
  const realFetch = globalThis.fetch
  globalThis.fetch = async () =>
    new Response(new ReadableStream({ start: (controller) => step(controller) }), { status: 200 })
  return () => {
    globalThis.fetch = realFetch
  }
}

// Settle with a fallback if the promise has not settled in time, so a
// reader with no deadline fails an assertion instead of hanging the suite.
function within(promise, ms, fallback) {
  return Promise.race([
    promise,
    new Promise((resolve) => setTimeout(() => resolve(fallback), ms)),
  ])
}

test('a stream that goes quiet after an event closes on the idle deadline', async () => {
  // The half-open socket: one frame arrives, then nothing, ever. The daemon
  // writes a keep-alive every 15 s, so silence past two of them means the
  // connection is not quiet, it is gone - and without a deadline the reader
  // waits forever while the chat sits frozen mid-turn.
  const restore = stubStream((controller) => {
    controller.enqueue(new TextEncoder().encode('id: 1\ndata: {"seq":1,"type":"a"}\n\n'))
  })
  try {
    const api = new DaemonApi('http://daemon', 'token')
    const closed = await within(
      new Promise((resolve) => {
        api.events('s1', {
          idleMs: 30,
          onEvent: () => {},
          onClose: (cursor, err) => resolve({ cursor, err }),
        })
      }),
      400,
      { cursor: 'never closed', err: null },
    )
    assert.equal(closed.cursor, 1)
    assert.equal(closed.err?.idle, true, 'an idle close says why, so the caller can re-dial')
  } finally {
    restore()
  }
})

test('a stream that keeps sending keep-alives is never declared idle', async () => {
  // The keep-alive is the daemon saying the wire is alive. A reader that
  // watched for events instead of bytes would re-dial every deadline here.
  let ticks = 0
  const restore = stubStream((controller) => {
    const timer = setInterval(() => {
      if (ticks++ < 5) {
        controller.enqueue(new TextEncoder().encode(': keep-alive\n\n'))
      } else {
        clearInterval(timer)
        controller.close()
      }
    }, 10)
  })
  try {
    const api = new DaemonApi('http://daemon', 'token')
    const closed = await within(
      new Promise((resolve) => {
        api.events('s1', {
          idleMs: 40,
          onEvent: () => {},
          onClose: (cursor, err) => resolve({ cursor, err }),
        })
      }),
      500,
      { cursor: 'never closed', err: null },
    )
    assert.equal(closed.cursor, null, 'a keep-alive folds nothing')
    assert.equal(closed.err, undefined, 'the wire was alive, so no idle close')
  } finally {
    restore()
  }
})

test('the idle deadline is two missed keep-alives and a margin', () => {
  assert.equal(KEEP_ALIVE_MS, 15000)
  assert.ok(STREAM_IDLE_MS > 2 * KEEP_ALIVE_MS)
  assert.ok(STREAM_IDLE_MS < 4 * KEEP_ALIVE_MS)
})

test('the re-dial wait backs off from 800 ms to a cap instead of a flat beat', () => {
  assert.equal(nextRetryDelay(undefined), 800)
  assert.equal(nextRetryDelay(0), 800)
  assert.equal(nextRetryDelay(800), 1600)
  assert.equal(nextRetryDelay(1600), 3200)
  assert.equal(nextRetryDelay(3200), 5000)
  // The cap holds however long the daemon stays down.
  assert.equal(nextRetryDelay(5000), 5000)
  assert.equal(nextRetryDelay(nextRetryDelay(5000)), 5000)
})

// ---- a session that is no longer there ----

test('a 404 close carries the status: the caller can tell a dead session from a dead connection', async () => {
  const { cursor, err } = await collect('', { fail: 404 })
  // No frame was ever read, so the resume cursor is the one asked for.
  assert.equal(cursor, null)
  assert.equal(err.status, 404)
  assert.ok(sessionGone(err))
})

test('another HTTP error also rides the close, but is not a gone session', async () => {
  const { err } = await collect('', { fail: 503 })
  assert.equal(err.status, 503)
  assert.ok(!sessionGone(err))
  // A clean close (stream simply ended) and no error at all: retryable.
  assert.ok(!sessionGone(undefined))
  assert.ok(!sessionGone(new Error('network drop')))
})

// ---- a probe the daemon could not ask ----

test('a failed models probe rejects: never an empty array to cache', async () => {
  // The daemon answers 503 when a harness's catalog probe fails (the
  // CLI not signed in, slow to boot, a spawn failure). An empty array is
  // a real answer callers may hold; a failed probe must never pose as
  // one, so it surfaces as the same rejection every failed call is.
  const api = new DaemonApi('http://daemon', 'token')
  const realFetch = globalThis.fetch
  globalThis.fetch = async () => new Response('{"error": "could not ask"}', { status: 503 })
  try {
    await assert.rejects(
      api.models('claude'),
      (err) => {
        assert.equal(err.message, 'could not ask')
        assert.equal(err.status, 503)
        return true
      },
    )
  } finally {
    globalThis.fetch = realFetch
  }
})

// ---- request timeouts ----

test('a request that never answers rejects when its timeout fires', async () => {
  // A socket that accepts the connection but never answers: the daemon
  // that is alive but wedged.
  const open = new Set()
  const server = createServer((socket) => {
    open.add(socket)
    socket.on('close', () => open.delete(socket))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()
  const api = new DaemonApi(`http://127.0.0.1:${port}`, 'token')
  try {
    await assert.rejects(api.call('GET', '/healthz', null, { timeoutMs: 200 }), /did not answer in time/)
  } finally {
    for (const socket of open) socket.destroy()
    server.close()
  }
})

test('every request is bounded: 10s normally, 120s across a harness boot, none for the installer', async () => {
  const api = new DaemonApi('http://daemon', 'token')
  const budgets = []
  const signals = []
  const realTimeout = AbortSignal.timeout
  const realFetch = globalThis.fetch
  // The budget is observable only where it is spent, so record what
  // reaches AbortSignal.timeout (and whether a signal rides the fetch).
  AbortSignal.timeout = (ms) => {
    budgets.push(ms)
    return new AbortController().signal
  }
  globalThis.fetch = async (requestUrl, options) => {
    signals.push(options.signal)
    return new Response('null', { status: 200 })
  }
  try {
    await api.call('GET', '/healthz')
    // The four whose wait includes a harness boot (a cold CLI can take
    // 30-60 s daemon-side): create, resume, run-now, and the model
    // catalog's CLI handshake.
    await api.createSession({ harness: 'claude' })
    await api.resume('s1')
    await api.runRule('r1')
    await api.models('claude')
    await api.installHarness('m1', 'claude')
  } finally {
    AbortSignal.timeout = realTimeout
    globalThis.fetch = realFetch
  }
  // The installer used to opt out of any client budget, and the two tests below
  // pinned that as the contract. Opting out left a daemon that was alive but wedged
  // holding the spinner forever; it has a budget now, the daemon's own ten minutes
  // plus a second, so the promise the exemption was there for still holds (the
  // client never gives up before the daemon answers) and the wait is bounded too.
  assert.deepEqual(budgets, [10000, 120000, 120000, 120000, 120000, 601000])
  assert.ok(signals.slice(0, 6).every((signal) => signal instanceof AbortSignal))
  // And the installer's signal is a real one now, not the absence of a bound.
  assert.ok(signals[5] instanceof AbortSignal)
})

// ---- a caller's own stop ----

test('a caller-aborted call rejects as a cancel, not as a timeout', async () => {
  // The folder picker stops its in-flight listing when a newer click
  // supersedes it: the raw abort must ride through (the stopper ignores
  // it by its signal, not by a message it cannot know), and it must
  // never masquerade as "the daemon did not answer in time".
  const api = new DaemonApi('http://daemon', 'token')
  const realFetch = globalThis.fetch
  let fetchSignal = null
  globalThis.fetch = async (requestUrl, options) => {
    fetchSignal = options.signal
    return new Promise((resolve, reject) => {
      fetchSignal.addEventListener('abort', () => reject(fetchSignal.reason), { once: true })
    })
  }
  const caller = new AbortController()
  try {
    const pending = api.call('GET', '/fs/dirs', null, { signal: caller.signal })
    await new Promise((resolve) => {
      if (fetchSignal) resolve()
      else setTimeout(resolve, 5)
    })
    caller.abort()
    await assert.rejects(pending, (err) => {
      assert.ok(!/did not answer in time/.test(err.message), 'a cancel is not a timeout')
      return true
    })
    // The caller's stop reached the wire.
    assert.equal(fetchSignal.aborted, true)
  } finally {
    globalThis.fetch = realFetch
  }
})

test('fsDirs rides the caller-supplied signal to the fetch, and an abort proves it', async () => {
  // The F9 wiring: `fsDirs(path, { signal })` must forward the stop to
  // the underlying call. call()'s own timeout hands fetch a fresh
  // AbortSignal on every request, so a signal merely being present
  // proves nothing - the proof is propagation: aborting the caller's
  // controller while the listing is in flight must abort the very
  // signal fetch received (dropped wiring fails the assert, not
  // silently).
  const api = new DaemonApi('http://daemon', 'token')
  const realFetch = globalThis.fetch
  const seen = []
  globalThis.fetch = async (requestUrl, options) => {
    seen.push(options.signal)
    if (seen.length === 1) {
      // Hold the first response until its signal aborts. The fallback
      // timer releases the hold when no stop ever came, so broken
      // wiring fails the assert below instead of hanging the suite.
      await new Promise((resolve) => {
        if (options.signal.aborted) return resolve()
        const timer = setTimeout(resolve, 250)
        options.signal.addEventListener(
          'abort',
          () => {
            clearTimeout(timer)
            resolve()
          },
          { once: true },
        )
      })
    }
    return new Response('null', { status: 200 })
  }
  const controller = new AbortController()
  try {
    const pending = api.fsDirs('C:/somewhere', { signal: controller.signal })
    await new Promise((resolve) => setTimeout(resolve, 5)) // the listing is in flight
    controller.abort()
    await pending
    await api.fsDirs(null) // an ordinary listing, no caller stop
  } finally {
    globalThis.fetch = realFetch
  }
  assert.equal(seen.length, 2)
  assert.ok(seen.every((signal) => signal instanceof AbortSignal))
  assert.equal(seen[0].aborted, true, "the caller's stop reached the wire")
  assert.equal(seen[1].aborted, false, 'an ordinary listing is never aborted')
  assert.ok(seen[0] !== seen[1], 'each call builds its own signal')
})

test('a device-scoped client streams that device, not this daemon', async () => {
  // events() builds its own URL and is the one request path that
  // bypassed the device prefix, so a watched chat on another computer
  // streamed this daemon's log instead.
  const urls = []
  const realFetch = globalThis.fetch
  globalThis.fetch = async (url) => {
    urls.push(url)
    const body = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(''))
        controller.close()
      },
    })
    return new Response(body, { status: 200 })
  }
  try {
    const api = new DaemonApi('http://daemon', 'token').device('machine-7')
    const streamed = (after) =>
      new Promise((resolve) => {
        api.events('s1', { after, onEvent: () => {}, onClose: () => resolve() })
      })
    await streamed(null)
    await streamed(4)
  } finally {
    globalThis.fetch = realFetch
  }
  assert.deepEqual(urls, [
    'http://daemon/devices/machine-7/sessions/s1/events',
    'http://daemon/devices/machine-7/sessions/s1/events?after=4',
  ])
})

// ---- the same daemon, reported twice ----

test('an address is normalised once, so two spellings are one daemon', () => {
  assert.equal(normaliseUrl('http://127.0.0.1:5175/'), 'http://127.0.0.1:5175')
  assert.equal(normaliseUrl('127.0.0.1:5175'), 'http://127.0.0.1:5175')
  assert.equal(normaliseUrl('https://daemon.example//'), 'https://daemon.example')
  assert.equal(normaliseUrl(null), null)
  // The client dials the same normalised form, which is why the comparison
  // below can be a plain equality.
  assert.equal(new DaemonApi('127.0.0.1:5175/', 'token').url, 'http://127.0.0.1:5175')
})

test('a status report for the same daemon is recognised, a changed one is not', () => {
  // The shell reports the daemon on start, on failure and on a respawn. The
  // same address and token is the same daemon: a reconnect would tear down
  // every stream for nothing.
  const api = new DaemonApi('http://127.0.0.1:5175', 'token')
  assert.equal(sameDaemon(api, 'http://127.0.0.1:5175', 'token'), true)
  assert.equal(sameDaemon(api, '127.0.0.1:5175/', 'token'), true, 'the same address, typed another way')
  assert.equal(sameDaemon(api, 'http://127.0.0.1:5176', 'token'), false, 'a respawn on a new port')
  assert.equal(sameDaemon(api, 'http://127.0.0.1:5175', 'other-token'), false, 'a new token')
  assert.equal(
    sameDaemon(new DaemonApi(null, null), 'http://127.0.0.1:5175', 'token'),
    false,
    'a first connection is not the same daemon',
  )
})

// ---- an id cannot move the route ----

test('a hostile id cannot change the route a call lands on', async () => {
  const seen = []
  const real = globalThis.fetch
  globalThis.fetch = async (url) => {
    seen.push(String(url))
    return new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } })
  }
  try {
    const api = new DaemonApi('http://127.0.0.1:9', 'token')
    await api.session('a/../../automations/abc')
    await api.deleteSession('../plugins/p')
    await api.answer('d/../../machines/m', 'allow')
    await api.device('../sessions/other').session('x')
  } finally {
    globalThis.fetch = real
  }
  // The route each call lands on is the one the method names: the id is data
  // inside one segment, so `..` cannot walk out of it.
  assert.deepEqual(
    seen.map((url) => new URL(url).pathname),
    [
      '/sessions/a%2F..%2F..%2Fautomations%2Fabc',
      '/sessions/..%2Fplugins%2Fp',
      '/decisions/d%2F..%2F..%2Fmachines%2Fm/answer',
      '/devices/..%2Fsessions%2Fother/sessions/x',
    ],
  )
})

// ---- a body the daemon refused ----

test('a 413 names the limit instead of the code', async () => {
  const real = globalThis.fetch
  globalThis.fetch = async () => new Response('', { status: 413, statusText: 'Payload Too Large' })
  try {
    const api = new DaemonApi('http://127.0.0.1:9', 'token')
    await assert.rejects(
      () => api.prompt('s1', 'a very long paste'),
      (err) => {
        assert.equal(err.status, 413)
        assert.doesNotMatch(err.message, /^HTTP 413$/, 'the bare code is not the sentence')
        assert.match(err.message, /too long/)
        assert.match(err.message, /at most 2\.0 MB/)
        return true
      },
    )
  } finally {
    globalThis.fetch = real
  }
})

// ---- pictures riding a message, and coming back (br-images 2b770c1) ----

const recording = (respond) => {
  const seen = []
  const fetchImpl = async (url, init) => {
    seen.push({ url, init })
    return respond(url, init)
  }
  return { seen, fetchImpl }
}

test('prompt, queue and steer carry the pictures as attachments, and leave the key out without any', async () => {
  const { seen, fetchImpl } = recording(() => new Response('{"ok":true}', { status: 200 }))
  const api = new DaemonApi('http://127.0.0.1:9', 'token', '', fetchImpl)
  const pics = [{ kind: 'image', mime: 'image/png', data: 'iVBORw0KGgo=' }]
  await api.prompt('s1', 'look', [], pics)
  await api.queue('s1', '', ['s2'], pics)
  await api.steer('s1', 'now', [], pics)
  await api.prompt('s1', 'words only')
  const bodies = seen.map((r) => JSON.parse(r.init.body))
  assert.deepEqual(
    seen.map((r) => new URL(r.url).pathname),
    ['/sessions/s1/prompts', '/sessions/s1/queue', '/sessions/s1/steer', '/sessions/s1/prompts'],
  )
  for (const body of bodies.slice(0, 3)) assert.deepEqual(body.attachments, pics)
  assert.equal(bodies[1].text, '', 'a message of pictures alone sends empty words')
  assert.deepEqual(bodies[1].mentioned_chats, ['s2'])
  assert.equal('attachments' in bodies[3], false)
})

test('a 413 on a message with pictures names the cross-device cap, not the text bound', async () => {
  const { fetchImpl } = recording(() => new Response('', { status: 413 }))
  const api = new DaemonApi('http://127.0.0.1:9', 'token', '', fetchImpl)
  await assert.rejects(
    () => api.prompt('s1', 'look', [], [{ kind: 'image', mime: 'image/png', data: 'AAAA' }]),
    (err) => {
      assert.equal(err.voice, 'console')
      assert.match(err.message, /another device/)
      assert.match(err.message, /4 MB/)
      assert.doesNotMatch(err.message, /too long/)
      return true
    },
  )
  // Words alone keep the text bound's sentence.
  await assert.rejects(() => api.queue('s1', 'long'), /too long/)
})

test("the daemon's refusal of pictures keeps its own words and status", async () => {
  const said = "Grok Build doesn't read pictures - send the message without them"
  const { fetchImpl } = recording(() => new Response(JSON.stringify({ error: said }), { status: 409 }))
  const api = new DaemonApi('http://127.0.0.1:9', 'token', '', fetchImpl)
  await assert.rejects(
    () => api.steer('s1', '', [], [{ kind: 'image', mime: 'image/png', data: 'AAAA' }]),
    (err) => err.status === 409 && err.message === said,
  )
})

test("a picture is fetched with the token, through the chat's own route on another device", async () => {
  const { seen, fetchImpl } = recording(
    () => new Response(new Uint8Array([1, 2, 3]), { status: 200, headers: { 'Content-Type': 'image/png' } }),
  )
  const api = new DaemonApi('http://127.0.0.1:9', 'token', '', fetchImpl)
  const here = await api.blob('a'.repeat(64))
  assert.equal(here.type, 'image/png')
  assert.equal(here.size, 3)
  await api.device('d1').blob('b'.repeat(64), { session: 's1', mime: 'image/png' })
  assert.deepEqual(
    seen.map((r) => new URL(r.url).pathname),
    [`/blobs/${'a'.repeat(64)}`, `/devices/d1/sessions/s1/blobs/${'b'.repeat(64)}`],
  )
  for (const r of seen) assert.equal(r.init.headers.Authorization, 'Bearer token')
})

test("a picture past another device's 16 MiB answer says so, with its status", async () => {
  const { fetchImpl } = recording(() => new Response('{"error":"too large"}', { status: 413 }))
  const api = new DaemonApi('http://127.0.0.1:9', 'token', '/devices/d1', fetchImpl)
  await assert.rejects(
    () => api.blob('c'.repeat(64), { session: 's1' }),
    (err) => err.status === 413 && /too large to bring over/.test(err.message),
  )
})

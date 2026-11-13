// The polling rules: one poll at a time, a hidden tab does nothing, a visible
// one catches up at once, and a failing machine is asked less often rather than
// hammered. Pure, so each rule can be read without a browser.

import assert from 'node:assert/strict'
import test from 'node:test'
import { POLL_MAX_MS, POLL_MS, mayPoll, pollDelay, visibilityAction } from './poll.js'

test('a poll may only start when the chat is open, visible and not already asking', () => {
  assert.equal(mayPoll({ open: true, visible: true }), true)
  assert.equal(mayPoll({ open: false, visible: true }), false)
  assert.equal(mayPoll({ open: true, visible: false }), false, 'a hidden tab does not poll')
  assert.equal(mayPoll({ open: true, visible: true, inFlight: true }), false, 'never two at once')
  assert.equal(mayPoll({ open: true, visible: true, stopped: true }), false, 'a deleted chat stays stopped')
  assert.equal(mayPoll(), false)
})

test('a hidden tab pauses and a returning one catches up immediately', () => {
  assert.equal(visibilityAction('hidden'), 'pause')
  assert.equal(visibilityAction('visible'), 'catch-up')
})

test('failures back off to a ceiling and success returns to the normal pace', () => {
  assert.equal(pollDelay(0), POLL_MS)
  assert.ok(pollDelay(1) > POLL_MS)
  assert.ok(pollDelay(3) >= pollDelay(2))
  assert.equal(pollDelay(99), POLL_MAX_MS)
  assert.ok(POLL_MAX_MS > 0)
})

test('the running poller waits for completion, backs off, pauses and cleans up', async () => {
  const { startPolling } = await import('./poll.js')
  let visibility
  const page = { visibilityState: 'visible', addEventListener: (_event, fn) => { visibility = fn }, removeEventListener: () => { visibility = null } }
  let scheduled = null
  let delay
  let finish
  let calls = 0
  const stop = startPolling(() => { calls++; return new Promise((resolve, reject) => { finish = { resolve, reject } }) }, {
    document: page, interval: 1000, maxDelay: 8000,
    setTimeout: (fn, ms) => { scheduled = fn; delay = ms; return 1 }, clearTimeout: () => { scheduled = null },
  })
  assert.equal(calls, 1)
  visibility()
  assert.equal(calls, 1, 'returning while in flight cannot overlap a request')
  assert.equal(scheduled, null)
  finish.reject(new Error('offline'))
  await Promise.resolve()
  assert.equal(delay, 2000)
  page.visibilityState = 'hidden'
  visibility()
  assert.equal(scheduled, null)
  page.visibilityState = 'visible'
  visibility()
  assert.equal(calls, 2, 'returning catches up immediately')
  finish.resolve()
  await Promise.resolve()
  assert.equal(delay, 1000, 'success resets backoff')
  stop()
  assert.equal(scheduled, null)
  assert.equal(visibility, null)
})

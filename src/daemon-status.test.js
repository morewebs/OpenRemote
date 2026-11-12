// The waiting screen's copy, pinned. It is the only thing the app says
// about a daemon that is not there, so a claim here is a claim the code
// has to keep: the title follows the failure the shell reported, the retry
// button exists only where daemon_restart does, and the detail line is
// whatever the shell said, cleaned.

import assert from 'node:assert/strict'
import test from 'node:test'

import { daemonStatus, detailOf } from './daemon-status.js'

const failure = (kind, text, extra = {}) => ({ error: { kind, text }, ...extra })

test('no answer yet is the starting state, with a way to start it again', () => {
  for (const info of [null, undefined, {}, { mobile: false }]) {
    const status = daemonStatus(info)
    assert.equal(status.tone, 'starting')
    assert.equal(status.title, "Waiting for OpenRemote's background service")
    assert.equal(status.detail, null)
    assert.equal(status.action, 'try-now')
    // A moment is not a promise of forever, and it is not an error.
    assert.match(status.note, /usually takes a moment/)
    assert.doesNotMatch(status.note, /did not start/)
  }
})

test("a daemon that never ran says so, in the shell's own words", () => {
  const reason = 'failed to spawn `openremote-daemon`: The system cannot find the file specified. (os error 2)'
  const status = daemonStatus(failure('start', reason))
  assert.equal(status.tone, 'failed')
  assert.equal(status.title, "OpenRemote's background service did not start")
  assert.equal(status.detail, reason)
  assert.equal(status.action, 'try-now')
  // The shell does retry the spawn on its own (desktop.rs backs off to
  // 30s), so the copy may say it does.
  assert.match(status.note, /keeps trying on its own/)
})

test('a daemon that ran and exited gets its own title', () => {
  const text = 'the daemon stopped (exit code 1)'
  for (const mobile of [false, true]) {
    const status = daemonStatus(failure('stopped', text, { mobile }))
    assert.equal(status.tone, 'failed')
    assert.equal(status.title, "OpenRemote's background service stopped")
    assert.doesNotMatch(status.title, /did not start/)
    assert.equal(status.detail, text)
  }
  // On the desktop it is being respawned, so the note says so and the
  // button is there.
  const desktop = daemonStatus(failure('stopped', text))
  assert.equal(desktop.action, 'try-now')
  assert.match(desktop.note, /keeps trying on its own/)
})

test('a daemon that is alive and quiet is not described as retried', () => {
  const status = daemonStatus(failure('silent', 'it started and has not answered in 90 seconds'))
  assert.equal(status.tone, 'failed')
  assert.equal(status.title, "OpenRemote's background service is running but not answering")
  assert.equal(status.action, 'try-now')
  // Nothing is retrying this one - the shell is waiting on the same process
  // - and nothing proves it is wedged either, so the note says both and says
  // what the button costs.
  assert.doesNotMatch(status.note, /keeps trying/)
  assert.match(status.note, /may still be finishing/)
  assert.match(status.note, /Try now restarts it/)
})

test('a reason the rule cannot read claims no more than it can', () => {
  // An unknown kind is the worst case, not the best one.
  assert.equal(daemonStatus(failure('nonsense', 'boom')).title, "OpenRemote's background service did not start")
  assert.equal(daemonStatus(failure('toString', 'boom')).title, "OpenRemote's background service did not start")
  // A reason in the older shape is still a reason.
  assert.equal(daemonStatus({ error: 'a plain reason' }).detail, 'a plain reason')
  // And a blank one is none at all: no empty detail box.
  for (const error of [
    { kind: 'start', text: '' },
    { kind: 'start', text: '   ' },
    { kind: 'start', text: '\n\t ' },
    { kind: 'start', text: null },
    { text: 42 },
    null,
  ]) {
    const status = daemonStatus({ error })
    assert.equal(status.tone, 'starting')
    assert.equal(status.detail, null)
  }
})

test('mobile gets no retry button, because daemon_restart is not there', () => {
  for (const kind of ['start', 'stopped', 'silent']) {
    const failed = daemonStatus(failure(kind, 'a reason', { mobile: true }))
    assert.equal(failed.tone, 'failed')
    assert.equal(failed.action, null)
    assert.match(failed.note, /Close OpenRemote and open it again/)
  }
  const starting = daemonStatus({ mobile: true })
  assert.equal(starting.action, null)
  assert.match(starting.note, /usually takes a moment/)
})

test('the detail is one line and never longer than the cap', () => {
  const messy = 'error:\r\n  could not open file\r\n\tat C:\\Program Files\\OpenRemote'
  assert.equal(detailOf(messy), 'error: could not open file at C:\\Program Files\\OpenRemote')
  const long = 'x'.repeat(1000)
  const capped = detailOf(long)
  assert.equal(capped.length, 240)
  assert.ok(capped.endsWith('...'))
  assert.equal(detailOf(null), null)
  assert.equal(daemonStatus(failure('start', messy)).detail, detailOf(messy))
})

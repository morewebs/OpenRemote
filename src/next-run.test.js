// When a rule will next fire, from the same local calendar the daemon's
// engine reads. The pure timestamp is asserted; labels are asserted only
// where the wording is relative, so a locale's clock format stays free.

import assert from 'node:assert/strict'
import test from 'node:test'
import { nextRunAt, nextRunLabel } from './next-run.js'

// A fixed local morning; every case builds its dates in the same calendar.
const now = new Date(2026, 10, 9, 9, 0, 0).getTime()
const at = (day, hour, minute) => new Date(2026, 10, day, hour, minute, 0).getTime()
const daily = (time) => ({ trigger: { kind: 'schedule', time }, created_at: now })
const interval = (minutes, lastRun, createdAt = now) => ({
  trigger: { kind: 'schedule', every_minutes: minutes },
  last_run: lastRun,
  created_at: createdAt,
})

test('a daily rule points at its next time, today or tomorrow', () => {
  assert.equal(nextRunAt(daily('10:00'), now), at(9, 10, 0))
  assert.equal(nextRunAt(daily('08:00'), now), at(10, 8, 0))
  // Exactly now has just fired; the next one is tomorrow, not today.
  assert.equal(nextRunAt(daily('09:00'), now), at(10, 9, 0))
})

test('an interval points one cadence after the run it last recorded', () => {
  assert.equal(nextRunAt(interval(30, now - 10 * 60_000), now), now + 20 * 60_000)
  // No run yet: the cadence counts from creation, not from zero.
  assert.equal(nextRunAt(interval(30, null, now - 10 * 60_000), now), now + 20 * 60_000)
  // Overdue is still a real time: the clock fires within one tick.
  assert.equal(nextRunAt(interval(30, now - 40 * 60_000), now), now - 10 * 60_000)
})

test('a webhook and an unreadable schedule have no next run', () => {
  assert.equal(nextRunAt({ trigger: { kind: 'webhook' } }, now), null)
  assert.equal(nextRunAt(daily('9am'), now), null)
  assert.equal(nextRunAt({ trigger: { kind: 'schedule' } }, now), null)
  assert.equal(nextRunAt({}, now), null)
  assert.equal(nextRunAt(interval(0, now), now), null)
  assert.equal(nextRunAt(interval('nope', now), now), null)
})

test('the label is relative while it is near, and names the clock otherwise', () => {
  assert.equal(nextRunLabel(interval(30, now - 10 * 60_000), now), 'next in 20 min')
  assert.equal(nextRunLabel(interval(30, now - 40 * 60_000), now), 'due now')
  assert.match(nextRunLabel(daily('10:00'), now), /^next /)
  assert.equal(nextRunLabel({ trigger: { kind: 'webhook' } }, now), null)
  // A rule that is off will not fire, whatever its clock says.
  assert.equal(nextRunAt({ ...daily('10:00'), enabled: false }, now), null)
})

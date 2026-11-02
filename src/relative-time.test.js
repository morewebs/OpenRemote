// A captured model catalog carries the moment it was captured, and the picker
// says how old it is instead of printing a timestamp. The rule is pure so the
// picker and its test read the same words: a moment ago is "just now", days
// are counted, and anything older than a month names the day, because by then
// the day is the thing a person is checking.

import assert from 'node:assert/strict'
import test from 'node:test'
import { relativeTime } from './relative-time.js'

test('a capture from two days ago reads as two days ago', () => {
  assert.equal(
    relativeTime(Date.parse('2026-10-31T10:00:00Z'), Date.parse('2026-11-02T10:00:00Z')),
    '2 days ago',
  )
})

test('a capture from last month names the day', () => {
  // Beyond a month the age stops being useful and the date starts being:
  // "33 days ago" is a number nobody converts.
  assert.equal(
    relativeTime(Date.parse('2026-09-30T10:00:00Z'), Date.parse('2026-11-02T10:00:00Z')),
    'on 2026-09-30',
  )
})

test('a missing capture is not a date', () => {
  // A harness that never captured has no time at all; epoch zero must not
  // read as 1970.
  assert.equal(relativeTime(0, Date.parse('2026-11-02T10:00:00Z')), 'not captured yet')
  assert.equal(relativeTime(null, Date.parse('2026-11-02T10:00:00Z')), 'not captured yet')
  assert.equal(relativeTime(undefined, Date.parse('2026-11-02T10:00:00Z')), 'not captured yet')
})

test('the age reads in the largest unit that fits', () => {
  const now = Date.parse('2026-11-02T10:00:00Z')
  assert.equal(relativeTime(now - 20_000, now), 'just now')
  assert.equal(relativeTime(now - 3 * 60_000, now), '3 minutes ago')
  assert.equal(relativeTime(now - 2 * 3_600_000, now), '2 hours ago')
  assert.equal(relativeTime(now - 6 * 86_400_000, now), '6 days ago')
  // One of a unit is singular, not "1 minutes ago".
  assert.equal(relativeTime(now - 60_000, now), '1 minute ago')
  assert.equal(relativeTime(now - 3_600_000, now), '1 hour ago')
  assert.equal(relativeTime(now - 86_400_000, now), '1 day ago')
})

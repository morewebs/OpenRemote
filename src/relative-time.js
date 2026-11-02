// How old a captured model catalog is, in words a person reads rather than a
// timestamp they have to convert. A capture older than a month names the day
// instead: "41 days ago" is a number nobody turns into a date. Kept pure so
// the picker's footer and this rule's test read the same words.

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
// Past this, the day itself is the useful fact.
const MONTH = 30 * DAY

// `at` is the daemon's capture time in epoch milliseconds; 0 or nothing means
// the harness never captured a catalog at all.
export function relativeTime(at, now = Date.now()) {
  const then = Number(at)
  if (!Number.isFinite(then) || then <= 0) return 'not captured yet'
  // A capture stamped in the future is a clock that disagreed with itself,
  // not a negative age: read it as the freshest thing there is.
  const age = Math.max(0, now - then)
  if (age < MINUTE) return 'just now'
  if (age < HOUR) return `${plural(Math.floor(age / MINUTE), 'minute')} ago`
  if (age < DAY) return `${plural(Math.floor(age / HOUR), 'hour')} ago`
  if (age < MONTH) return `${plural(Math.floor(age / DAY), 'day')} ago`
  return `on ${new Date(then).toISOString().slice(0, 10)}`
}

function plural(count, unit) {
  return `${count} ${unit}${count === 1 ? '' : 's'}`
}

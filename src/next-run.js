// When a rule will next fire, from the same local calendar the daemon's
// engine reads. A daily rule names its clock time; an interval rule counts
// from the run it last recorded, or from its creation. An overdue interval
// is due on the engine's next tick, so it says so rather than naming a time
// in the past.

const TIME = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' })
const DAY = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' })

// A stamp from storage: a number in one version, the digits a JSON echo made
// in another. Zero, a non-number and a non-finite value are not a time.
function stamp(value) {
  if (value == null || value === '') return null
  const n = Number(value)
  return Number.isFinite(n) && n > 0 ? n : null
}

/**
 * The rule's next scheduled fire in milliseconds, or null when it has none:
 * a webhook, a disabled rule, an unreadable time, or an interval whose base
 * was never recorded.
 */
export function nextRunAt(rule, now = Date.now()) {
  if (rule?.enabled === false) return null
  const trigger = rule?.trigger
  if (trigger?.kind !== 'schedule') return null
  if (trigger.every_minutes != null) {
    const every = Number(trigger.every_minutes)
    if (!Number.isFinite(every) || every <= 0) return null
    const base = stamp(rule.last_run) ?? stamp(rule.created_at)
    if (base == null) return null
    return base + every * 60_000
  }
  const match = /^(\d{2}):(\d{2})$/.exec(trigger.time ?? '')
  if (!match) return null
  const hour = Number(match[1])
  const minute = Number(match[2])
  if (hour > 23 || minute > 59) return null
  const target = new Date(now)
  target.setHours(hour, minute, 0, 0)
  // The minute has already passed, so the next one is tomorrow. A missed
  // day is not replayed: the engine only fires on its own minute.
  if (target.getTime() <= now) target.setDate(target.getDate() + 1)
  return target.getTime()
}

/**
 * One row fragment: a short relative sentence while the fire is near, the
 * wall-clock time otherwise, and "due now" for an interval already past.
 * Null means there is nothing true to say.
 */
export function nextRunLabel(rule, now = Date.now()) {
  const at = nextRunAt(rule, now)
  if (at == null) return null
  if (at <= now) return 'due now'
  const minutes = Math.ceil((at - now) / 60_000)
  if (minutes < 60) return `next in ${minutes} min`
  const next = new Date(at)
  const today = new Date(now)
  const clock = TIME.format(next)
  return next.toDateString() === today.toDateString()
    ? `next ${clock}`
    : `next ${DAY.format(next)}, ${clock}`
}

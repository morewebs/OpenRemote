// The polling rules for the browser's device view, as pure functions.
//
// Over the link there is no stream: the console asks a machine for a range of a
// chat's log and asks again later. That makes the schedule a rule worth
// pinning: one poll at a time, faster while a send is waiting, backing off on
// failure, paused when the tab is hidden, and one immediate catch-up when it
// comes back.

/// How long between polls while a chat is open and the tab visible. The daemon
/// keeps no push channel here, so this is the freshness a person gets.
export const POLL_MS = 1500

/// The longest wait between attempts after failures: long enough to stop
/// hammering a machine that is asleep, short enough that a recovered link is
/// noticed while the chat is still on screen.
export const POLL_MAX_MS = 15000

/// The wait before the next poll. A run of failures doubles the base wait up to
/// the ceiling; success resets the run, so the next poll is the normal pace.
export function pollDelay(failures = 0) {
  if (!(failures > 0)) return POLL_MS
  const doubled = POLL_MS * 2 ** Math.min(failures, 8)
  return Math.min(doubled, POLL_MAX_MS)
}

/// Whether a poll may start now. One at a time is the whole point: two pumps
/// would fold the same page twice and double the transcript's live blocks.
export function mayPoll({ open = false, stopped = false, visible = true, inFlight = false } = {}) {
  return Boolean(open) && !stopped && visible && !inFlight
}

/// What a visibility change means. A hidden tab does no work; a tab coming
/// back asks at once rather than waiting out the old timer, because the person
/// is looking at the transcript again.
export function visibilityAction(state) {
  return state === 'hidden' ? 'pause' : 'catch-up'
}

// Completion-based scheduling prevents slow links from accumulating requests.
export function startPolling(task, {
  interval = POLL_MS, maxDelay = POLL_MAX_MS,
  document: page = globalThis.document,
  setTimeout: later = globalThis.setTimeout, clearTimeout: cancel = globalThis.clearTimeout,
} = {}) {
  let stopped = false
  let inFlight = false
  let timer = null
  let failures = 0
  const visible = () => page?.visibilityState !== 'hidden'
  const tick = async () => {
    if (stopped || inFlight || !visible()) return
    inFlight = true
    try { await task(); failures = 0 } catch { failures++ } finally {
      inFlight = false
      if (!stopped && visible()) timer = later(tick, Math.min(interval * 2 ** Math.min(failures, 8), maxDelay))
    }
  }
  const visibility = () => {
    cancel(timer)
    if (visible()) void tick()
  }
  page?.addEventListener('visibilitychange', visibility)
  void tick()
  return () => {
    stopped = true
    cancel(timer)
    page?.removeEventListener('visibilitychange', visibility)
  }
}

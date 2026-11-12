// What the app says while it has no daemon. Pure, so the copy can be held
// to what the shell actually reports: the shell sends a reason only when
// it has one (desktop.rs clears it the moment a daemon says READY, and
// never sets one over a live address), so the rule only has to tell
// "there is a reason" from "there is none", and which one it is.

// A reason is the shell's own sentence. It is honest and it is the only
// diagnostic the app has, so it is shown, cleaned and capped rather than
// hidden behind a generic line.
const MAX_DETAIL = 240

export function detailOf(text) {
  if (typeof text !== 'string') return null
  const line = text.replace(/\s+/g, ' ').trim()
  if (!line) return null
  return line.length > MAX_DETAIL ? `${line.slice(0, MAX_DETAIL - 3)}...` : line
}

// A daemon that never ran, one that ran and exited, and one that is alive
// and quiet are three different pieces of news. Only the last is not
// already being retried by the shell, so the second sentence differs.
const TITLES = {
  start: "OpenRemote's background service did not start",
  stopped: "OpenRemote's background service stopped",
  silent: "OpenRemote's background service is running but not answering",
}

const RETRIED = 'It keeps trying on its own, every half minute at worst. Try now skips the wait.'

const NOTES = {
  start: RETRIED,
  stopped: RETRIED,
  // Nothing is retrying this process, and nothing proves it is stuck: a
  // first run on a busy machine can take this long and still finish. Try
  // now is a restart, so the sentence has to say what it costs.
  silent: 'It may still be finishing. Try now restarts it.',
}

/**
 * The waiting screen's whole content. `info` is the shell's DaemonInfo, or
 * null before the first answer. `action` is 'try-now' exactly where a
 * person can act: on the desktop, where daemon_restart exists. Android
 * runs the daemon in-process and has no such command, so it is told to
 * reopen the app instead of being offered a button that would do nothing.
 */
export function daemonStatus(info) {
  const error = info?.error
  // One shape from the shell (a kind plus its words), and the older one,
  // a bare string: a reason is never silently dropped, because the panel
  // would go back to waiting with nothing to show.
  const detail = detailOf(typeof error === 'string' ? error : error?.text)
  const mobile = !!info?.mobile
  if (detail) {
    // No kind at all is not a reason to claim less than the worst case: a
    // reason without one is treated as a daemon that never started.
    const kind = Object.hasOwn(TITLES, error?.kind) ? error.kind : 'start'
    return {
      tone: 'failed',
      title: TITLES[kind],
      detail,
      note: mobile ? 'Close OpenRemote and open it again.' : NOTES[kind],
      action: mobile ? null : 'try-now',
    }
  }
  return {
    tone: 'starting',
    title: "Waiting for OpenRemote's background service",
    detail: null,
    note: 'It starts with the app and usually takes a moment.',
    action: mobile ? null : 'try-now',
  }
}

// What the shell says when the daemon is not answering. Pure: the shell
// passes the connection's own state and the two facts it cannot know from
// the state alone (did this session ever connect, and can this build restart
// the daemon), and gets back the row's words or nothing at all.
//
// Three different pieces of news live near here, and they must not be
// confused: a daemon that has not answered yet, one that answered and then
// stopped, and an account that is signed out. Only the first two are the
// daemon's fault. Signing out is not: the Cloud door and each Cloud view say
// it in their own words, so this row is never built from the account at all
// and cannot blame the daemon for it.

// The console's own sentence already says a daemon that answers with an
// error has done so in its words; the note falls back to these when the
// connection carries no words of its own.
const STOPPED = 'It may have exited. Starting it again usually brings the lists back.'
const STOPPED_MANUAL = 'It may have exited. Close OpenRemote and open it again.'
const NEVER = 'It starts with the app and usually takes a moment. Starting it again is the quickest way in.'
const NEVER_MANUAL = 'It starts with the app. Open OpenRemote again if it does not appear.'

/// The longest sentence the row will quote from the connection. A reason is
/// shown rather than hidden, but a row is one line: it is capped, not cut
/// mid-word where a word boundary is anywhere near the cap.
const MAX_DETAIL = 160

export function detailOf(text) {
  if (typeof text !== 'string') return null
  const line = text.replace(/\s+/g, ' ').trim()
  if (!line) return null
  if (line.length <= MAX_DETAIL) return line
  const cut = line.slice(0, MAX_DETAIL - 3)
  const space = cut.lastIndexOf(' ')
  return `${space > MAX_DETAIL / 2 ? cut.slice(0, space) : cut}...`
}

/**
 * The row, or null when there is nothing to say.
 *
 * `state` is the console's connection state; `everConnected` is whether this
 * window has had a working connection since it opened; `restartable` is
 * whether this build can start the daemon again (the desktop shell can, a
 * phone runs it in-process); `showsOwnError` is true when the current view
 * already prints the same news in its own error line, so the row stays out
 * of its way; `error` is the connection's own words, quoted when it has any.
 */
export function connectionNotice({
  state,
  everConnected = false,
  restartable = false,
  showsOwnError = false,
  error = null,
} = {}) {
  // A working connection has nothing to say, and neither do the two states
  // that are not a fault yet: 'connecting' and 'new' are the normal first
  // moments of a session, and 'waiting-daemon' replaces the whole body with
  // the daemon's own screen, which already carries the news and the button.
  if (state !== 'error') return null
  // A chat's transcript and the New chat home print the daemon sentence in
  // their own error line; a second copy above them is the same news twice.
  if (showsOwnError) return null
  const detail = detailOf(error)
  if (everConnected) {
    return {
      tone: 'stopped',
      title: "OpenRemote's background service stopped answering",
      note: detail ?? (restartable ? STOPPED : STOPPED_MANUAL),
      action: restartable ? 'restart' : null,
    }
  }
  return {
    tone: 'never',
    title: "OpenRemote is not connected to its background service",
    note: detail ?? (restartable ? NEVER : NEVER_MANUAL),
    action: restartable ? 'restart' : null,
  }
}

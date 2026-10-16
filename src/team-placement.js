// Where a chat shows up next to another. A team bar (a lead's workers) and
// an "Started by" chip (a worker's lead) ask for the other chat to open
// beside this one; the app's shell answers, since it owns the panes - so a
// chat never needs to know the layout. (The same shape as
// browser-placement.js.)

const handlers = new Set()

/** The shell listens for chats asking to show another chat. */
export function onBesideRequest(fn) {
  handlers.add(fn)
  return () => handlers.delete(fn)
}

/**
 * Show `chat` beside `from`: in a pane of its own on the desktop (the pane
 * that already shows it, if one does), in the main view on the phone.
 */
export function requestBeside({ from, chat }) {
  for (const fn of handlers) fn({ from, chat })
}

// What a chat asks of the app around it: open a view, start a new chat set
// up like this one. A chat's / commands (/plugins, /new) reach the app this
// way, the same way popups.js lets anything open Settings.

const listeners = new Set()

// kind: 'view' ({ id }) or 'new-chat' ({ chatId }).
export function requestApp(kind, payload = {}) {
  for (const fn of listeners) fn(kind, payload)
}

export function onAppRequest(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

// A page, a picture of one or a picked element the user shares with a chat
// from the browser puts the browser in play for that chat, as an agent's own
// browser call does: the daemon lists that chat's agent the browser tools
// from then on (/sessions/{id}/browser/shared). The home screen's composer
// has no chat yet, so its share waits for the chat it becomes.

import { browserCall } from './browser-store.js'
import { composerChatId } from './composer-scope.js'

const waiting = new Set()

/** The share reached composer `key` (a chat's id, or 'new'). */
export function sharedWith(key, tell) {
  if (!key) return
  if (composerChatId(key) === 'new') {
    waiting.add(key)
    return
  }
  tell(composerChatId(key))
}

/** The home screen's composer became chat `id`: tell it of a share made there. */
export function chatMade(id, tell, key = 'new') {
  if (!waiting.has(key)) return
  waiting.delete(key)
  tell(id)
}

/** Test-only: forget a waiting share. */
export function forgetShares() {
  waiting.clear()
}

/** Tells this computer's daemon; a chat it does not hold (one on another
 *  device) is refused, and that is no reason to bother the user. */
export function tellDaemon(id) {
  return browserCall('POST', `/sessions/${encodeURIComponent(id)}/browser/shared`, {}).catch(() => {})
}

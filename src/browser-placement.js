// Where the browser shows up next to a chat. On the desktop it opens as a
// pane beside the chat - when the user asks (a chat's Browser button, an
// "Open beside chat" chip), or by itself when the agent of the chat in
// the main pane opens a tab; other chats never steal the view. On the
// phone it never switches views by itself: the chat shows a pill instead.
//
// Chats ask through this module and the app's shell answers (it owns the
// panes), so a chat never needs to know the layout.

const handlers = new Set()

/** The shell listens for chats asking for the browser. */
export function onBrowserRequest(fn) {
  handlers.add(fn)
  return () => handlers.delete(fn)
}

/**
 * Show the browser for a chat: beside it on the desktop, the Browser view
 * on the phone. With `url`, that address opens in a new tab; with `tab`,
 * the browser shows that tab.
 */
export function requestBrowser({ chatId, url = null, tab = null, action = null, target = null }) {
  for (const fn of handlers) fn({ chatId, url, tab, ...(action ? { action, target } : {}) })
}

const BROWSER_TOOL = 'mcp__openremote__browser_'

/**
 * Whether a browser:tab event should open the browser beside the main
 * pane's chat by itself: the tab's controller is that chat's agent, and
 * this tab hasn't opened it for that chat before - a page loading or
 * changing its title fires the event again, and a pane the user closed
 * stays closed until the agent opens another tab.
 */
export function shouldAutoOpen(tab, primaryChat, opened) {
  const controller = tab?.controller
  if (!controller || controller.kind !== 'agent' || !primaryChat) return false
  if (controller.chat !== primaryChat) return false
  return !opened.has(openedKey(tab))
}

/** The key `shouldAutoOpen` remembers a tab by. */
export function openedKey(tab) {
  return `${tab.id}|${tab.controller?.chat ?? ''}`
}

/**
 * Whether a chat's agent is using the browser now: its turn is running
 * and the last tool it called was one of the browser's - read from the
 * transcript, so every device showing the chat can tell.
 */
export function isBrowsing(chat) {
  if (!chat || (chat.status !== 'running' && chat.status !== 'waiting')) return false
  for (let i = chat.timeline.length - 1; i >= 0; i--) {
    const item = chat.timeline[i]
    if (item.kind === 'tool') return typeof item.name === 'string' && item.name.startsWith(BROWSER_TOOL)
    if (item.kind === 'message' && item.role === 'user') return false
  }
  return false
}

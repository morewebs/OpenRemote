// The browser's tab list as the console keeps it: order, which tab is
// active, closing and reopening. A tab is { key, native, url, title,
// loading }: `native` is the shell's webview for it, null while the tab
// shows the new tab page. Pure: tested without a DOM.

let counter = 0

export function newTab(fields = {}) {
  counter += 1
  return { key: `t${Date.now().toString(36)}${counter}`, native: null, url: '', title: '', loading: false, ...fields }
}

// Close a tab: the one to its right becomes active (Chrome's way), else
// the one to its left. The last tab closing leaves a new tab page, so the
// browser is never empty.
export function closeTab(tabs, key, active) {
  const at = tabs.findIndex((t) => t.key === key)
  if (at === -1) return { tabs, active }
  const rest = tabs.filter((t) => t.key !== key)
  if (!rest.length) {
    const fresh = newTab()
    return { tabs: [fresh], active: fresh.key, closed: tabs[at] }
  }
  const next = active === key ? (rest[at] ?? rest[at - 1]).key : active
  return { tabs: rest, active: next, closed: tabs[at] }
}

// The next or previous tab, wrapping around.
export function cycle(tabs, active, step) {
  if (!tabs.length) return active
  const at = Math.max(0, tabs.findIndex((t) => t.key === active))
  return tabs[(at + step + tabs.length) % tabs.length].key
}

// Insert a tab right after another (a popup beside its opener), or at the
// end.
export function insertAfter(tabs, tab, afterKey) {
  const at = tabs.findIndex((t) => t.key === afterKey)
  if (at === -1) return [...tabs, tab]
  return [...tabs.slice(0, at + 1), tab, ...tabs.slice(at + 1)]
}

// The title a tab shows: the page's own, else its address, else "New tab".
export function tabTitle(tab) {
  if (tab.title) return tab.title
  if (tab.url && tab.url !== 'about:blank') return tab.url.replace(/^https?:\/\//, '')
  return 'New tab'
}

// Whether a page that has just finished opening still has somewhere to be:
// a tab in the list that points at it and a browser pane still on screen.
// Both can go while the shell builds the page (a close, Ctrl+W, the pane's
// X). A page that lands anyway has no tab strip to close it with and floats
// over the console, so it is closed instead of shown.
export function openBelongs(tabs, key, mounted) {
  return Boolean(mounted) && tabs.some((t) => t.key === key)
}

// Whether a page a tab opened (a popup, from a link with target=_blank or
// window.open) belongs to a tab this console has. The tab that opened it is
// what places the new one, so a popup whose opener is not in the list has
// nowhere to land: it would stay in the shell's tab list with no console tab,
// and come back as a stray tab on the next console load.
export function popupBelongs(tabs, opener) {
  return tabs.some((t) => t.native && t.native === opener)
}

// ---- tabs waiting as entries, and the session saved for the next start ----

// A tab waiting for its page: it has an address but no page yet, and is
// not opening one or refused one. Restored tabs and tabs put away wait like
// this until they are shown.
export function isDormant(tab) {
  return Boolean(tab?.url) && tab.url !== 'about:blank' && !tab.native && !tab.loading && !tab.failed
}

// Only web addresses come back: anything else a page reached is not worth
// reopening at a start, and nothing that runs script ever is.
const RESTORABLE = /^(https?|file):/i

// The tabs saved for the next start: every tab with an address, never a
// private one, and which of them was showing.
export function sessionOf(tabs, active) {
  const kept = (tabs ?? []).filter((t) => !t.private && t.url && RESTORABLE.test(t.url))
  const at = kept.findIndex((t) => t.key === active)
  return { tabs: kept.map((t) => ({ url: t.url, title: t.title ?? '' })), active: Math.max(0, at) }
}

// A saved session as tabs again: entries with their address and title,
// which open their pages when first shown. Anything malformed is dropped.
export function restoredTabs(session) {
  const saved = Array.isArray(session?.tabs) ? session.tabs : []
  const tabs = saved
    .filter((t) => t && typeof t.url === 'string' && RESTORABLE.test(t.url))
    .map((t) => newTab({ url: t.url, title: typeof t.title === 'string' ? t.title : '' }))
  const at = Number.isInteger(session?.active) ? session.active : 0
  return { tabs, active: Math.min(Math.max(0, at), Math.max(0, tabs.length - 1)) }
}

// The tab list once the last session is back: the restored tabs first, then
// any that opened meanwhile (an agent's, one the user is already in); the
// empty new tab page the browser started with gives way. One the user is in
// stays shown, else the session's shown tab is.
export function withRestored(tabs, active, restored) {
  if (!restored.tabs.length) return { tabs, active }
  const kept = tabs.filter((t) => t.native || t.url)
  const busy = kept.some((t) => t.key === active)
  return { tabs: [...restored.tabs, ...kept], active: busy ? active : restored.tabs[restored.active].key }
}

// ---- putting pages away ----

// Pages kept open at once. Past this the least recently shown are put away
// to entries that open again when shown: each open page holds memory, an
// entry holds an address.
export const LIVE_LIMIT = 10

// When more than `limit` pages are open, the ones to put away: the least
// recently shown first (`seen`), never the tab on screen, one an agent
// holds, or a private one (putting it away would end its private session).
export function toDiscard(tabs, active, limit = LIVE_LIMIT) {
  const live = (tabs ?? []).filter((t) => t.native)
  const extra = live.length - limit
  if (extra <= 0) return []
  return live
    .filter((t) => t.key !== active && !t.controller && !t.private)
    .sort((a, b) => (a.seen ?? 0) - (b.seen ?? 0))
    .slice(0, extra)
}

// A tab put away: its address and title stay, its page and what the page
// said about itself go.
export function putAway(tab) {
  return { ...tab, native: null, loading: false, canBack: false, canForward: false, find: null }
}

// ---- the closed tabs a window remembers ----
//
// A closed tab leaves its address and title, never its shell page: that page
// went with the close. Five is a short memory on purpose - it is the tab you
// just lost, not a history - and the list lives in the browser's store, one
// per window, so it never travels to another device.
//
// A tab that had nowhere to be (the new tab page, about:blank) leaves nothing:
// reopening it would ask the browser to restore nothing. Closing the same
// address twice keeps one entry, so one reopen is enough to get back to it.

export const CLOSED_CAP = 5

/// What a closing tab leaves for the list, or null when there is nothing to
/// reopen. A private tab leaves no trace, the reopen list included.
export function closedEntry(tab) {
  if (!tab || tab.private || !tab.url || tab.url === 'about:blank') return null
  return { url: tab.url, title: tab.title || tab.url }
}

/// The list after a tab closes: newest first, capped, no repeated address.
export function rememberClosed(closed, tab) {
  const entry = closedEntry(tab)
  if (!entry) return closed
  const rest = (closed ?? []).filter((e) => e.url !== entry.url)
  return [entry, ...rest].slice(0, CLOSED_CAP)
}

/// What reopening does to the list: the newest entry, and the list behind it.
/// `null` when the list is empty (the command has nothing to do).
export function takeClosed(closed) {
  if (!closed?.length) return null
  return { entry: closed[0], rest: closed.slice(1) }
}

// ---- closing several at once ----
//
// Both commands work on the tab list in strip order, not on the tabs that
// happen to be visible at this width: a close that did less after a window
// resize would be the same command doing two different things. The visible
// strip is where the menu is asked, not what it is allowed to touch.

/// Every tab but the one asked from. One tab closes nothing.
export function closeOthers(tabs, key) {
  if (!tabs.some((t) => t.key === key)) return []
  return tabs.filter((t) => t.key !== key).map((t) => t.key)
}

/// Every tab after this one in strip order. The last tab closes nothing.
export function closeToRight(tabs, key) {
  const at = tabs.findIndex((t) => t.key === key)
  if (at === -1) return []
  return tabs.slice(at + 1).map((t) => t.key)
}

// ---- how much of the strip fits ----
//
// A tab keeps its width and is never squeezed to a sliver: what does not fit
// moves behind one overflow control.
//
// These numbers are the sheet's numbers, named by the selector each one comes
// from, because the plan and the CSS drifting apart is how a strip starts
// drawing tabs that are not there. browser-strip.test.js reads browser.css and
// asserts each constant equals the width it claims:
//
//   TAB_WIDTH          .bw-tab { width: 200px }
//   TAB_GAP            .bw-tabs-scroll { gap: 2px }
//   NEW_TAB_WIDTH      .bw-newtab { width: 28px }
//
// The overflow control (.bw-tabsearch.is-more, 56px) sits beside the measured
// strip, not inside it, so its width is not part of this plan and it is not a
// constant here: the sheet owns it, and browser-strip.test.js reads the sheet's
// value directly.
//
// The floor is the tab's own minimum: one tab and the gap before it and the
// new-tab button, 230px at these numbers. Nothing in split.css gives a pane a
// minimum width, so a pane can in principle be dragged narrower than that; the
// plan still answers one tab and the CSS clips its right edge. That clip is
// accepted - the alternative is a shrunken tab, which is the sliver this rule
// exists to prevent.

export const TAB_WIDTH = 200
export const TAB_GAP = 2
export const NEW_TAB_WIDTH = 28

/// Which tabs the strip draws: a window of `shown` tabs starting at `start`,
/// always containing the active one, with `hidden` behind the control. An
/// unmeasured strip (width 0, the first paint) shows everything.
///
/// A strip narrower than one tab plus the button (230px at these numbers)
/// still answers one tab: the strip is a pane and nothing in it may shrink, so
/// below that width the CSS clips the tab's right edge rather than pretending
/// Whether this build's shell implements a browser feature. `features` is the
/// shell's own answer or null when the ask never landed; unknown is not yes,
/// because a control the shell would refuse must not be drawn as if it worked.
/// The view names each refusal, so a false here is what makes the control say
/// "not in this build" instead of doing nothing when pressed.
export function canUse(features, name) {
  return Boolean(features && features[name])
}

/// Whether the shell answered and has none of the extras (find in page, the
/// element picker, device view, forgetting a site). Used for the one line the
/// view says when the browser is otherwise working without them.
export function extrasMissing(features) {
  if (features == null) return false
  return !Object.values(features).some(Boolean)
}

/// the tab is not there.
export function stripPlan({ width = 0, count = 0, active = 0, tabWidth = TAB_WIDTH, gap = TAB_GAP } = {}) {
  if (count <= 0) return { start: 0, shown: 0, hidden: 0 }
  if (width <= 0) return { start: 0, shown: count, hidden: 0 }
  // Each tab is followed by a gap, and the new-tab button is the last thing in
  // the row, so `n` tabs occupy `n` tab widths, `n` gaps and the button.
  const room = (n) => n * (tabWidth + gap) + NEW_TAB_WIDTH
  let shown = 0
  if (room(count) <= width) {
    shown = count
  } else {
    for (let n = count - 1; n >= 1; n -= 1) {
      if (room(n) <= width) {
        shown = n
        break
      }
    }
    shown = Math.max(1, shown)
  }
  const start = Math.min(Math.max(0, active - shown + 1), count - shown)
  return { start, shown, hidden: count - shown }
}

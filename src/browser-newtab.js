// The new tab page's Recent list: the addresses this window has read, newest
// first, one row per address, a handful. It reads browser-history.js's record
// and nothing else: no fetch, no account, and a private tab is never recorded
// there (see the pane's visit effect), so it can never appear here.
//
// Why six: the page is a search box with a glanceable list under it. Six fills
// two short rows at the page's own width, and a longer list would push the page
// and turn the new tab into a history screen, which the pane's menu already has.

import { bareAddress } from './browser-omnibox.js'

// The number the page draws. Exported so the test pins it and the page cannot
// quietly grow into a second history list.
export const RECENT_CAP = 6

/**
 * The rows the new tab page shows, from the history list (newest first):
 * one per address, keeping the first visit's title, at most `cap` of them.
 * A row with no address is not a row; a row with no title is drawn by its
 * address (visitLabel, the same rule the history list uses).
 */
export function recentFor(history, { cap = RECENT_CAP } = {}) {
  const limit = Number.isInteger(cap) && cap > 0 ? cap : RECENT_CAP
  const seen = new Set()
  const rows = []
  for (const entry of history ?? []) {
    const url = String(entry?.url ?? '')
    if (!url || seen.has(url)) continue
    seen.add(url)
    rows.push({ url, title: String(entry?.title ?? '').trim() })
    if (rows.length >= limit) break
  }
  return rows
}

// ---- shortcuts and the dev-server row ----
//
// Chrome's shortcuts under the search box: the user's own first, then the
// sites read most (the daemon's GET /browser/tiles ranks them), eight at
// most. And one quiet row of the dev servers seen in chats (devservers.js),
// six at most. A private tab's page shows neither.

export const TILE_CAP = 8
export const SERVER_CAP = 6

/** The tiles to draw from the daemon's answer: rows with an address, eight at most. */
export function tilesFrom(answer) {
  if (!Array.isArray(answer)) return []
  return answer
    .filter((t) => t && typeof t.url === 'string' && t.url)
    .map((t) => ({ url: t.url, title: String(t.title ?? '').trim(), pinned: Boolean(t.pinned) }))
    .slice(0, TILE_CAP)
}

function hostName(url) {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return ''
  }
}

/** A tile's name: the one it was given, or its site. */
export function tileLabel(tile) {
  return tile?.title || hostName(tile?.url) || bareAddress(tile?.url)
}

/** The letter a tile wears in place of a picture. */
export function tileLetter(tile) {
  return (hostName(tile?.url) || bareAddress(tile?.url) || '?').charAt(0).toUpperCase()
}

/** Whether "Add shortcut" shows: the user's own shortcuts have room left. */
export function canAddTile(tiles) {
  return (tiles ?? []).filter((t) => t.pinned).length < TILE_CAP
}

/** The dev-server row: newest first, six at most, each drawn by its bare address. */
export function serverRows(servers) {
  return (servers ?? []).slice(0, SERVER_CAP).map((s) => ({ url: s.url, label: bareAddress(s.url), chat: s.chat ?? null }))
}

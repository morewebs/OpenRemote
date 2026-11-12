// The right-click menu's pure rules (ContextMenu.jsx draws them): which rows
// the keys land on, how the arrows walk them, and where a row's own list - a
// submenu - opens. Pure, so node:test reads them without a renderer.

// How long a submenu stays open after the pointer moves on to another row:
// long enough to cross the menu's other rows on the way into it.
export const SUBMENU_LINGER_MS = 250

/** The rows the keys land on: an action, or a row that opens its own list. */
export function menuActions(items) {
  return (items ?? [])
    .map((item, i) => ({ item, i }))
    .filter(({ item }) => (item.onSelect || item.submenu) && !item.disabled)
}

/**
 * Where an arrow, Home or End moves the cursor among `list` ({ i } rows)
 * from position `at` (-1: nothing under the cursor yet). `null` for any
 * other key, or an empty list.
 */
export function stepCursor(list, at, key) {
  const n = list.length
  if (n === 0) return null
  if (key === 'ArrowDown') return list[(at + 1) % n].i
  if (key === 'ArrowUp') return list[at === -1 ? n - 1 : (at - 1 + n) % n].i
  if (key === 'Home') return list[0].i
  if (key === 'End') return list[n - 1].i
  return null
}

/**
 * Where a submenu opens: beside its row, to the right where there is room,
 * else to the left. On a phone there is room on neither side, so it opens
 * over the menu, just under its row. Always inside the window.
 */
export function submenuPlace({ menu, row, list, vw, vh }) {
  let left = menu.right + 4
  let top = row.top - 7
  if (left + list.width > vw - 8) left = menu.left - 4 - list.width
  if (left < 8) {
    left = Math.max(8, Math.min(row.left + 16, vw - list.width - 8))
    top = row.bottom + 2
  }
  top = Math.max(8, Math.min(top, vh - list.height - 8))
  return { left, top }
}

// The sidebar's list rules, pure so they are tested without a DOM: what a pin
// does to a list, how many chats a project shows before "Show N more", and
// where a pinned row sits while the rest of the list is paged.
//
// Pins are this device's own arrangement (the same shelf as the seen map and
// the folded projects): the list order is newest activity first, and a pinned
// chat leaves that order for the top of its own project in the order it was
// pinned, newest pin first, so a pin is where it was just put. A pin whose chat
// is gone - deleted, archived, or moved to another project - is skipped by the
// list it is not in and appears in the list it moved to, because each group is
// planned from its own rows.

import { heldStates } from './projects.js'

const PIN_KEY = 'openremote-pinned-chats'

// Five per project: a head, its chats and the next head within one glance; a
// sixth row is where the column starts pushing the next project out of view.
// The flat Chats list is the catch-all, so it pages at fifteen, the number the
// design used before this rule existed.
export const PROJECT_PAGE = 5
export const CHATS_PAGE = 15

/** The pinned ids, newest first, repaired: strings only, no duplicates. */
export function loadPins() {
  try {
    const raw = JSON.parse(localStorage.getItem(PIN_KEY) ?? '[]')
    if (!Array.isArray(raw)) return []
    const out = []
    for (const id of raw) if (typeof id === 'string' && id && !out.includes(id)) out.push(id)
    return out
  } catch {
    return []
  }
}

export function storePins(pins) {
  try {
    localStorage.setItem(PIN_KEY, JSON.stringify((pins ?? []).filter((id) => typeof id === 'string' && id)))
  } catch {
    /* storage unavailable: the pins last this run */
  }
}

/**
 * The list after a press on "Pin" or "Unpin": a new pin goes to the front
 * (newest first), an old one leaves, and a press with no id is the same list.
 */
export function togglePin(pins, id) {
  const list = Array.isArray(pins) ? pins : []
  if (typeof id !== 'string' || !id) return list
  return list.includes(id) ? list.filter((x) => x !== id) : [id, ...list]
}

/** The rows among `rows` that a pin names, in pin order; a pin with no row is skipped. */
export function pinnedRows(rows, pins) {
  const byId = new Map((rows ?? []).map((row) => [row.id, row]))
  return (pins ?? []).map((id) => byId.get(id)).filter(Boolean)
}

/**
 * One list as the sidebar draws it. `items` are the group's rows in list
 * order; a pinned row leaves them for `pinned`, so it is never drawn twice.
 * The rest show the first `page` of them, or all of them once the person asked
 * for more (`expanded`, for this session); a row `keep` names - the chat that
 * is open - stays visible wherever it sits. `next` is how many rows are left
 * out, so the "Show N more" line never names a number the press will not show,
 * and `hidden` are those rows, for the line's hint (`moreHint`).
 */
export function planList(items, { pins = [], page = PROJECT_PAGE, expanded = false, keep = () => false } = {}) {
  const list = items ?? []
  const pinnedIds = new Set(pins ?? [])
  const pinned = pinnedRows(list, pins)
  const rest = list.filter((row) => !pinnedIds.has(row.id))
  const limit = expanded ? rest.length : Math.max(0, page)
  const shown = rest.filter((row, i) => i < limit || keep(row))
  const shownIds = new Set(shown.map((row) => row.id))
  const hidden = rest.filter((row) => !shownIds.has(row.id))
  return { pinned, shown, hidden, next: hidden.length }
}

/**
 * What the rows a "Show N more" line leaves out are up to, as the parts the
 * line draws after its count - `{ kind: 'failed' | 'running', text }` - or
 * null when none of them failed or runs. A long list never hides a failure
 * behind its page (ported from br-sidebar b941184's dot on the line, in the
 * words a folded project's head uses). Waiting chats are never paged away:
 * they sit in the group above.
 */
export function moreHint(rows) {
  const { failed, running } = heldStates(rows)
  const parts = []
  if (failed > 0) parts.push({ kind: 'failed', text: `${failed} failed` })
  if (running > 0) parts.push({ kind: 'running', text: `${running} running` })
  return parts.length ? parts : null
}

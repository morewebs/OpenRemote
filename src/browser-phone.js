// The phone browser's small rules (BrowserPhone.jsx): where the native page
// sits, which page is on screen, what the address and the menus say. Pure:
// tested without a DOM or a phone.

// How far out of view a page waits while a menu covers it: the same size,
// so it comes back without a relayout.
export const PARKED_X = -30000

/** A box on whole device pixels: its edges rounded, not its size, so the native page meets the console's own pixels. */
export function snapBox(box, dpr = 1) {
  const px = (v) => Math.round(v * dpr) / dpr
  const x = px(box.x)
  const y = px(box.y)
  return { x, y, width: px(box.x + box.width) - x, height: px(box.y + box.height) - y }
}

/**
 * The page area for the phone's shell (`browser_set_bounds`), in CSS pixels:
 * the desktop's box plus how far the page may slide up under the bar as it
 * scrolls (none while finding, which holds the bar), the screen's scale,
 * whether motion is wanted, and whether Android's back belongs to the page.
 * Null for an area that is not laid out.
 */
export function pageBounds(rect, { dpr = 1, bar = 0, still = false, finding = false, parked = false, bottom = false } = {}) {
  if (!rect || !(rect.width > 0) || !(rect.height > 0)) return null
  const box = snapBox({ x: rect.left, y: rect.top, width: rect.width, height: rect.height }, dpr)
  return {
    ...box,
    x: parked ? PARKED_X : box.x,
    bar: finding ? 0 : bar,
    scale: dpr,
    still: Boolean(still),
    back: !finding,
    // Which side of the page the bar is on: at the bottom the page grows down
    // into the room the bar leaves instead of sliding up under it.
    edge: bottom ? 'bottom' : 'top',
  }
}

/** Where the bar is drawn while the page has slid `offset` pixels: up off the top, or down off the bottom. */
export function barTransform(offset, bottom = false) {
  if (!(offset > 0)) return ''
  return `translateY(${bottom ? offset : -offset}px)`
}

/** The shell's page that should be on screen: the tab's, unless the console covers it or the tab shows its new tab page. */
export function shownPage(tab, covered) {
  if (covered || !tab?.native || tab.home) return null
  return tab.native
}

/** Whether the tab grid may picture a tab: a private tab's page is never pictured, so no picture of it exists to keep. */
export function thumbWanted(tab) {
  return Boolean(tab?.native) && !tab.private
}

/** The grid's two sections, in strip order. */
export function gridSections(tabs) {
  return { normal: (tabs ?? []).filter((t) => !t.private), kept: (tabs ?? []).filter((t) => t.private) }
}

export function siteOf(url) {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return ''
  }
}

/** What the closed address bar says: the site, or the invitation to type. */
export function addressLine(tab) {
  if (!tab?.native || tab.home) return 'Search or type a web address'
  return siteOf(tab.url) || tab.url || 'Search or type a web address'
}

/** The tab a popup came from, which back returns to. */
export function openerTab(tabs, tab) {
  if (!tab?.opener) return null
  return (tabs ?? []).find((t) => t.native === tab.opener) ?? null
}

/**
 * What a long press on a link or a picture offers, in order. A private tab
 * offers no "open in private tab": everything it opens is private already.
 */
export function contextActions(context, inPrivate) {
  if (!context) return []
  const actions = []
  if (context.link) {
    actions.push('open')
    if (!inPrivate) actions.push('private')
    actions.push('copy', 'share')
  }
  if (context.kind === 'image') actions.push('image', 'download', 'copy-image')
  else actions.push('download')
  return actions
}

function sizeOf(bytes) {
  if (!(bytes > 0)) return ''
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** A download's state in words. */
export function downloadLine(d) {
  if (d?.state === 'started') {
    if (d.total > 0 && d.received >= 0) return `Downloading · ${Math.round((d.received / d.total) * 100)}%`
    return d.received > 0 ? `Downloading · ${sizeOf(d.received)}` : 'Downloading'
  }
  if (d?.state === 'failed') return "Couldn't download"
  if (d?.state === 'cancelled') return 'Cancelled'
  return sizeOf(d?.total || d?.received) || 'Done'
}

/** The line under the bar while a chat's agent holds the tab. */
export function agentLine(controller) {
  if (!controller) return ''
  return controller.kind === 'paused' ? `You took over from "${controller.title}"` : `"${controller.title}" is using this tab`
}

// ---- swiping the bar between tabs ----

// How far a finger must travel sideways along the bar to change tabs, and
// how much more sideways than up or down the travel must be.
export const SWIPE_MIN = 56
const SWIPE_SLOPE = 1.5

/**
 * The tab a sideways swipe along the bar goes to, or null: Chrome's toolbar
 * swipe. A swipe to the left goes to the tab after this one, to the right to
 * the one before; it stays among tabs of the same kind (normal or private)
 * and stops at the ends.
 */
export function swipeTab(tabs, active, dx, dy = 0) {
  if (!(Math.abs(dx) >= SWIPE_MIN) || Math.abs(dx) < Math.abs(dy) * SWIPE_SLOPE) return null
  const current = (tabs ?? []).find((t) => t.key === active)
  if (!current) return null
  const same = tabs.filter((t) => Boolean(t.private) === Boolean(current.private))
  const at = same.findIndex((t) => t.key === active)
  const next = same[at + (dx < 0 ? 1 : -1)]
  return next ? next.key : null
}

// ---- tab groups ----

/** The colours a group may wear, in the order new groups take them. */
export const GROUP_COLORS = ['blue', 'amber', 'green', 'red', 'purple', 'teal', 'pink', 'grey']

/** A new group's fields: the first colour no group wears yet. */
export function newGroup(groups, id) {
  const worn = new Set(Object.values(groups ?? {}).map((g) => g.color))
  const color = GROUP_COLORS.find((c) => !worn.has(c)) ?? GROUP_COLORS[Object.keys(groups ?? {}).length % GROUP_COLORS.length]
  return { id, name: '', color }
}

/** What a group is called on its card: its name, or how many tabs it holds. */
export function groupLabel(group, count) {
  const name = String(group?.name ?? '').trim()
  if (name) return name
  return `${count} ${count === 1 ? 'tab' : 'tabs'}`
}

/**
 * The normal section of the grid: tabs on their own, and each group as one
 * card where its first tab stands. A tab naming a group that no longer exists
 * is drawn on its own. Private tabs are not here (gridSections keeps them).
 */
export function gridEntries(tabs, groups) {
  const entries = []
  const placed = new Map()
  for (const tab of tabs ?? []) {
    if (tab.private) continue
    const group = tab.group && groups?.[tab.group]
    if (!group) {
      entries.push({ kind: 'tab', key: tab.key, tab })
      continue
    }
    if (placed.has(tab.group)) {
      placed.get(tab.group).tabs.push(tab)
      continue
    }
    const entry = { kind: 'group', key: `group:${tab.group}`, group, tabs: [tab] }
    placed.set(tab.group, entry)
    entries.push(entry)
  }
  return entries
}

/** The groups that still hold a tab: a group whose last tab closed is gone. */
export function liveGroups(groups, tabs) {
  const used = new Set((tabs ?? []).filter((t) => !t.private && t.group).map((t) => t.group))
  const kept = Object.fromEntries(Object.entries(groups ?? {}).filter(([id]) => used.has(id)))
  return Object.keys(kept).length === Object.keys(groups ?? {}).length ? groups : kept
}

// ---- Recent tabs ----

/**
 * The daemon's `/browser/devices` answer, as the page may draw it: devices
 * with a name and web pages only. Anything else the answer holds is dropped.
 */
export function devicesFrom(answer) {
  const rows = Array.isArray(answer?.devices) ? answer.devices : []
  return rows
    .filter((d) => d && typeof d.device === 'string')
    .map((d) => ({
      device: d.device,
      name: typeof d.name === 'string' && d.name.trim() ? d.name.trim() : 'Another device',
      at: Number.isFinite(d.at) ? d.at : 0,
      tabs: (Array.isArray(d.tabs) ? d.tabs : [])
        .filter((t) => t && typeof t.url === 'string' && /^https?:\/\//i.test(t.url))
        .map((t) => ({ url: t.url, title: typeof t.title === 'string' ? t.title : '' })),
    }))
    .filter((d) => d.tabs.length > 0)
}

/** How long ago, in Chrome's words. */
export function ago(at, now = Date.now()) {
  if (!(at > 0)) return ''
  const minutes = Math.floor(Math.max(0, now - at) / 60_000)
  if (minutes < 1) return 'Just now'
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} hr ago`
  const days = Math.floor(hours / 24)
  return days === 1 ? 'Yesterday' : `${days} days ago`
}

// ---- reading progress ----

/** How far down the page is, as a width for the bar's thin line (0 to 100), or 0 while it loads. */
export function readWidth(read, loading) {
  if (loading || !Number.isFinite(read)) return 0
  return Math.round(Math.min(1, Math.max(0, read)) * 1000) / 10
}

// ---- the menu ----

/**
 * The menu's rows, in Chrome's order. `live` is a page on screen, `isPrivate`
 * a private tab, `web` a page with a web address. Each row is its id and
 * whether it can be used now.
 */
export function menuRows({ live = false, isPrivate = false, web = false, grouped = false } = {}) {
  const rows = [
    ['new-tab', true],
    ['new-private', true],
  ]
  // Private tabs are never grouped; a grouped tab is in one already.
  if (!isPrivate) rows.push(['group', live && !grouped])
  rows.push('-', ['history', true], ['clear', true], '-', ['downloads', true], ['bookmarks', true], ['recent', true], '-')
  rows.push(['share', live], ['find', live], ['desktop', live])
  if (isPrivate) rows.push(['agents', live])
  rows.push(['external', live && web], '-', ['settings', true], ['ntp', true])
  return rows
}

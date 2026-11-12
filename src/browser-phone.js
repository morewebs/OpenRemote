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
export function pageBounds(rect, { dpr = 1, bar = 0, still = false, finding = false, parked = false } = {}) {
  if (!rect || !(rect.width > 0) || !(rect.height > 0)) return null
  const box = snapBox({ x: rect.left, y: rect.top, width: rect.width, height: rect.height }, dpr)
  return {
    ...box,
    x: parked ? PARKED_X : box.x,
    bar: finding ? 0 : bar,
    scale: dpr,
    still: Boolean(still),
    back: !finding,
  }
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

// What the browser hands a composer as a picture (attachments.js): a
// screenshot of the page, or of the part the user dragged over. Ported from
// the browser branch's browser-attach.js (0d3211a); a picked element keeps
// main's own path (browser-pick.js, as text through composer-insert.js).
// Pure: tested without a DOM.

import { hostOf } from './browser-chrome.js'

/** A picture of the page (or a part of it) as an image attachment. */
export function screenshotAttachment(dataUrl, url) {
  const host = hostOf(url).replace(/^www\./, '')
  return {
    kind: 'image',
    name: `Screenshot of ${host || 'the page'}.jpg`,
    mime: 'image/jpeg',
    dataUrl,
  }
}

/**
 * How many on-screen pixels one of the page's CSS pixels takes: the tab's
 * zoom, times the device view's fit when the page is drawn as a device.
 */
export function regionScale(zoom = 1, deviceScale = 1) {
  const z = Number(zoom) > 0 ? Number(zoom) : 1
  const d = Number(deviceScale) > 0 ? Number(deviceScale) : 1
  return z * d
}

/**
 * The part dragged over (`r`, in the page box's on-screen pixels) in the
 * page's own CSS pixels, from the top-left of what shows: what the shell's
 * browser_capture `clip` takes (it adds how far the page has scrolled).
 */
export function clipFor(r, scale) {
  const s = Number(scale) > 0 ? Number(scale) : 1
  return { x: r.x / s, y: r.y / s, width: r.width / s, height: r.height / s }
}

/**
 * Whether the picture the shell answered a clip with is the whole page
 * rather than the part: far wider than the clip on this screen, or a
 * different shape from what was dragged. The part is then cut here instead.
 */
export function cameWhole(picture, clip, r, dpr = 1) {
  if (!picture?.width || !picture?.height) return true
  if (picture.width > clip.width * dpr * 1.5 + 8) return true
  return Math.abs(picture.width / picture.height - r.width / r.height) > 0.05
}

/** A drag too small to mean a part (a click): the picker keeps waiting. */
export function tooSmall(r) {
  return !r || r.width < 4 || r.height < 4
}

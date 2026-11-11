// The browser chrome's small rules: device sizes, zoom steps, download
// names. Pure: tested without a DOM.

// Device view presets: CSS pixel sizes of common screens.
export const DEVICES = [
  { name: 'Phone', width: 393, height: 852, mobile: true },
  { name: 'Small phone', width: 360, height: 780, mobile: true },
  { name: 'Tablet', width: 820, height: 1180, mobile: true },
  { name: 'Laptop', width: 1280, height: 800, mobile: false },
]

// A device's size fitted into the space the page has: never larger, the
// same shape. `scale` is how far it was shrunk (1 = actual size).
export function fitDevice(device, area) {
  const scale = Math.min(1, area.width / device.width, area.height / device.height)
  const safe = Number.isFinite(scale) && scale > 0 ? scale : 1
  return { width: Math.round(device.width * safe), height: Math.round(device.height * safe), scale: safe }
}

// Chrome's zoom steps.
export const ZOOMS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5]

// The next zoom step in a direction: 1 in, -1 out, 0 back to 100%.
export function nextZoom(current, dir) {
  if (dir === 0) return 1
  if (dir > 0) return ZOOMS.find((z) => z > current + 0.001) ?? ZOOMS[ZOOMS.length - 1]
  return [...ZOOMS].reverse().find((z) => z < current - 0.001) ?? ZOOMS[0]
}

export function zoomLabel(factor) {
  return `${Math.round(factor * 100)}%`
}

// A download's name: the file's own, from its path or address.
export function fileName(pathOrUrl) {
  if (!pathOrUrl) return 'Download'
  let tail = String(pathOrUrl).split(/[\\/]/).filter(Boolean).pop() ?? ''
  tail = tail.split(/[?#]/)[0]
  try {
    tail = decodeURIComponent(tail)
  } catch {
    // a name that is not valid percent-encoding stays as written
  }
  return tail || 'Download'
}

/// What "Clear browsing data" asks the shell to clear (browser_clear_data's
/// kinds) for what was ticked: cookies come with the sites' storage, since a
/// site keeps its sign-in in both.
export function clearKinds({ cookies = false, cache = false } = {}) {
  return [...(cookies ? ['cookies', 'storage'] : []), ...(cache ? ['cache'] : [])]
}

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

/// Whether a key event belongs to the browser: its target is the browser's
/// own element (focused when the address bar lets go) or inside it. The page
/// body, or a chat beside the browser, keeps the app's keys.
export function browserHasFocus(root, target) {
  return Boolean(root && target && (root === target || root.contains(target)))
}


// ---- site permissions ----

// A page's host ("example.com:8080"), or '' for an address that has none.
export function hostOf(url) {
  try {
    return new URL(url).host
  } catch {
    return ''
  }
}

// What a site asks for, as the permission bar says it: "<host> wants to
// <ask>". The shell names the kind (win.rs `permission_kind`).
const ASKS = {
  camera: 'use your camera',
  microphone: 'use your microphone',
  geolocation: 'know your location',
  notifications: 'show notifications',
  'clipboard-read': 'see what you copy',
  midi: 'use your MIDI devices',
}

export function permissionAsk(kind, label) {
  return ASKS[kind] ?? (label ? `use ${label}` : 'use a feature of this computer')
}

// A remembered permission's name in a list.
const NAMES = {
  camera: 'Camera',
  microphone: 'Microphone',
  geolocation: 'Location',
  notifications: 'Notifications',
  'clipboard-read': 'Clipboard',
  midi: 'MIDI devices',
}

export function permissionName(kind) {
  const name = NAMES[kind] ?? String(kind ?? '').replace(/-/g, ' ')
  return name.charAt(0).toUpperCase() + name.slice(1)
}

// The remembered answers (the daemon's `origin|kind` -> allow or deny) as
// rows, sorted by site; only one site's when `origin` is given.
export function permissionRows(permissions, origin = null) {
  return Object.entries(permissions ?? {})
    .map(([key, state]) => {
      const cut = key.lastIndexOf('|')
      return { origin: key.slice(0, cut), kind: key.slice(cut + 1), state }
    })
    .filter((r) => r.origin && (r.state === 'allow' || r.state === 'deny') && (!origin || r.origin === origin))
    .sort((a, b) => a.origin.localeCompare(b.origin) || a.kind.localeCompare(b.kind))
}

// The answer remembered for a site's request, or null when the site must
// ask. A private tab never has one: it always asks and remembers nothing.
export function rememberedAnswer(permissions, { origin, kind, private: priv = false } = {}) {
  if (priv) return null
  const state = permissions?.[`${origin}|${kind}`]
  return state === 'allow' || state === 'deny' ? state : null
}

// A recording's file name in Downloads, from when it started:
// "OpenRemote recording 2026-11-11 14.05.09.webm" (no colons: Windows
// refuses them in a name).
export function recordingName(date) {
  const two = (n) => String(n).padStart(2, '0')
  const day = `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}`
  const time = `${two(date.getHours())}.${two(date.getMinutes())}.${two(date.getSeconds())}`
  return `OpenRemote recording ${day} ${time}.webm`
}

// A recording's running time as the pill shows it: "1:05".
export function recordingClock(startedAt, now = Date.now()) {
  const secs = startedAt ? Math.max(0, Math.floor((now - startedAt) / 1000)) : 0
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}`
}

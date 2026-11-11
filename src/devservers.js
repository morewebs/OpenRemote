// Dev servers a chat mentions: addresses on this computer with a port
// (http://localhost:5173, http://127.0.0.1:3000/api) in an agent's reply
// or a tool's output - what the user will want to open in the browser.
// Chats get an "Open beside chat" chip for each; the browser's new tab
// page lists the ones seen lately. Kept in memory only: a dev server is
// news while it runs, not history.

import { loadConnection } from './api/daemon.js'

const KEPT = 8
const seen = []
const listeners = new Set()

// localhost, 127.0.0.1, 0.0.0.0, [::1] and *.localhost names, each with a
// port (never followed by another digit), then any path. Matched
// case-insensitively, anywhere in the text; a path stops where quotes,
// brackets or markdown would close around it.
const ADDRESS =
  /\bhttps?:\/\/(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]|(?:[a-z0-9-]+\.)+localhost):(\d{1,5})(?!\d)(?:[/?#][^\s"'`<>)\]]*)?/gi

// What a sentence wraps around an address, never part of it.
const TRAILING = /[.,;:!?]+$/

// The app's own addresses: its daemon, and in development the console
// itself - never a dev server of the user's.
function ownPorts() {
  const ports = new Set()
  const url = loadConnection().url
  const daemon = url ? /:(\d{1,5})(?:\/|$)/.exec(url) : null
  if (daemon) ports.add(daemon[1])
  if (typeof window !== 'undefined' && /^https?:$/.test(window.location.protocol) && window.location.port) {
    ports.add(window.location.port)
  }
  return ports
}

function originOf(url) {
  try {
    return new URL(url).origin
  } catch {
    return url
  }
}

/**
 * The dev server addresses in `text`, in order, each once:
 * `[{ url, host, port }]`. 0.0.0.0 (a server listening everywhere) opens
 * as localhost, which a browser can reach.
 */
export function findDevServers(text) {
  if (!text || typeof text !== 'string') return []
  const own = ownPorts()
  const found = []
  const urls = new Set()
  for (const match of text.matchAll(ADDRESS)) {
    const host = match[1].toLowerCase()
    const port = match[2]
    if (Number(port) < 1 || Number(port) > 65535 || own.has(port)) continue
    let url = match[0].replace(TRAILING, '')
    if (host === '0.0.0.0') url = url.replace(/0\.0\.0\.0/, 'localhost')
    if (urls.has(url)) continue
    urls.add(url)
    found.push({ url, host, port: Number(port) })
  }
  return found
}

function changed() {
  for (const fn of listeners) fn()
}

/** Remember the dev servers a chat's text mentions, newest first. */
export function noteDevServers(chatId, text) {
  const found = findDevServers(text)
  if (!found.length) return
  const now = Date.now()
  for (const { url } of found) {
    const origin = originOf(url)
    const at = seen.findIndex((s) => s.url === origin)
    if (at !== -1) seen.splice(at, 1)
    seen.unshift({ url: origin, seenAt: now, chat: chatId })
  }
  seen.splice(KEPT)
  changed()
}

/** The dev servers seen lately, newest first, one per server: `[{ url, seenAt, chat }]`. */
export function recentDevServers() {
  return seen.map((s) => ({ ...s }))
}

/** Hear when the list changes; returns the way to stop. */
export function subscribeDevServers(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

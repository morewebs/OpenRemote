// The browser's own settings - the search engine, whether the bookmarks
// bar shows on every page - and the sites' remembered permissions, kept by
// the daemon (`/browser`, which stores only what was changed) and cached
// here for the app's lifetime.
// Settings > Browser and the open browser share the cache, so a change in
// one shows in the other at once. It asks the daemon only when one of them
// mounts, so the app pays nothing for it at start.

import { useSyncExternalStore } from 'react'
import { SEARCH_ENGINES } from './browser-omnibox.js'

// What each setting is until the user changes it:
//   search         a SEARCH_ENGINES key (browser-omnibox.js)
//   bookmarks_bar  the bar on every page; off, on the new tab page only
//   startup        'continue' (the last session's tabs come back at the
//                  first open after a start) or 'new_tab'
export const BROWSER_DEFAULTS = { search: 'google', bookmarks_bar: true, startup: 'continue' }

// What "On startup" offers, in Settings > Browser.
export const STARTUP_CHOICES = [
  { id: 'continue', name: 'Continue where you left off' },
  { id: 'new_tab', name: 'Open the new tab page' },
]

/// A setting's value: the saved one when it is one this console reads,
/// else the default. An engine this build no longer lists reads as Google.
export function browserSetting(settings, key) {
  const value = settings?.[key]
  if (value === undefined || value === null) return BROWSER_DEFAULTS[key]
  if (key === 'search' && !Object.hasOwn(SEARCH_ENGINES, value)) return BROWSER_DEFAULTS.search
  if (key === 'bookmarks_bar') return value === true
  if (key === 'startup' && !STARTUP_CHOICES.some((c) => c.id === value)) return BROWSER_DEFAULTS.startup
  return value
}

// An object the daemon answered, or an empty one.
function plain(value) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value : {}
}

// `permissions` is the daemon's `origin|kind` -> 'allow' | 'deny' map;
// `session` the tabs this device saved for the next start (or null).
let prefs = { loaded: false, settings: {}, permissions: {}, session: null }
const listeners = new Set()

function set(change) {
  prefs = { ...prefs, ...change }
  for (const fn of listeners) fn()
}

export function getBrowserPrefs() {
  return prefs
}

export function subscribeBrowserPrefs(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

export function useBrowserPrefs() {
  return useSyncExternalStore(subscribeBrowserPrefs, getBrowserPrefs)
}

/// The daemon's settings and remembered permissions, fetched again through
/// `call(method, path, body)` (browser-store's `browserCall`). A failed read
/// keeps the cache: the defaults stand until the daemon answers.
export async function loadBrowserPrefs(call) {
  try {
    const got = await call('GET', '/browser')
    if (got && typeof got === 'object') {
      set({
        loaded: true,
        settings: plain(got.settings),
        permissions: plain(got.permissions),
        session: got.session && typeof got.session === 'object' ? got.session : null,
      })
    }
  } catch {
    /* the cache stays as it was; the next mount asks again */
  }
  return prefs
}

/// One setting changed: shown at once, then written. A write the daemon
/// does not take puts the value back and throws, so the caller can say why.
export async function setBrowserPref(call, key, value) {
  const before = prefs.settings
  set({ settings: { ...before, [key]: value } })
  try {
    const settings = await call('POST', '/browser/settings', { [key]: value })
    if (settings && typeof settings === 'object') set({ settings })
  } catch (err) {
    set({ settings: before })
    throw err
  }
}

/// A site's answer remembered ('allow' or 'deny'), or forgotten (null) so the
/// site asks again. Shown at once; a write the daemon does not take puts the
/// list back and throws, so the caller can say why.
export async function setSitePermission(call, origin, kind, state) {
  const before = prefs.permissions
  const key = `${origin}|${kind}`
  const next = { ...before }
  if (state) next[key] = state
  else delete next[key]
  set({ permissions: next })
  try {
    const saved = await call('POST', '/browser/permissions', { origin, kind, state: state ?? null })
    if (saved && typeof saved === 'object') set({ permissions: plain(saved) })
  } catch (err) {
    set({ permissions: before })
    throw err
  }
}

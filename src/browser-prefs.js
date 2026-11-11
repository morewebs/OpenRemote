// The browser's own settings - the search engine, whether the bookmarks
// bar shows on every page - kept by the daemon (`/browser/settings`, which
// stores only what was changed) and cached here for the app's lifetime.
// Settings > Browser and the open browser share the cache, so a change in
// one shows in the other at once. It asks the daemon only when one of them
// mounts, so the app pays nothing for it at start.

import { useSyncExternalStore } from 'react'
import { SEARCH_ENGINES } from './browser-omnibox.js'

// What each setting is until the user changes it:
//   search         a SEARCH_ENGINES key (browser-omnibox.js)
//   bookmarks_bar  the bar on every page; off, on the new tab page only
export const BROWSER_DEFAULTS = { search: 'google', bookmarks_bar: true }

/// A setting's value: the saved one when it is one this console reads,
/// else the default. An engine this build no longer lists reads as Google.
export function browserSetting(settings, key) {
  const value = settings?.[key]
  if (value === undefined || value === null) return BROWSER_DEFAULTS[key]
  if (key === 'search' && !Object.hasOwn(SEARCH_ENGINES, value)) return BROWSER_DEFAULTS.search
  if (key === 'bookmarks_bar') return value === true
  return value
}

let prefs = { loaded: false, settings: {} }
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

/// The daemon's settings, fetched again through `call(method, path, body)`
/// (browser-store's `browserCall`). A failed read keeps the cache: the
/// defaults stand until the daemon answers.
export async function loadBrowserPrefs(call) {
  try {
    const settings = await call('GET', '/browser/settings')
    if (settings && typeof settings === 'object') set({ loaded: true, settings })
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

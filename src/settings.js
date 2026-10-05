// Console-side preferences, persisted locally. Pure functions on the
// document/location-shaped objects the caller passes, so tests run
// without a DOM. The reducer-shaped key set is open: a new pref is a
// field here plus an apply line in restoreBootPrefs.

const PREFS_KEY = 'openremote-prefs'
const RECENTS_KEY = 'openremote-recent-workspaces'
// Superseded by PREFS_KEY's reduce_motion; read only, for the migration.
const OLD_REDUCE_KEY = 'openremote-reduce-motion'

const DENSITIES = ['comfortable', 'compact']

export function loadPrefs() {
  let raw = {}
  try {
    raw = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}')
  } catch {
    raw = {}
  }
  const reduce =
    typeof raw.reduce_motion === 'boolean'
      ? raw.reduce_motion
      : localStorage.getItem(OLD_REDUCE_KEY) === '1'
  return {
    reduce_motion: reduce === true,
    density: DENSITIES.includes(raw.density) ? raw.density : 'comfortable',
    startup_view: raw.startup_view === 'last' ? 'last' : 'new',
  }
}

export function savePrefs(partial) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ ...loadPrefs(), ...partial }))
  } catch {
    /* storage unavailable */
  }
}

// The document-shaped surface these two functions touch: nothing but
// documentElement attributes. Tests pass a stub with the same shape.
export function applyReduceMotion(doc, value) {
  if (value) doc.documentElement.setAttribute('data-reduce-motion', '')
  else doc.documentElement.removeAttribute('data-reduce-motion')
}

export function applyDensity(doc, value) {
  doc.documentElement.setAttribute('data-density', value)
}

// Run once before first React paint: restores the persisted look and,
// when the user asked to reopen their last view, seeds the hash the
// router already knows how to validate. A garbage last-view is left
// for loadViewState's known-set to fall back from - never a crash.
export function restoreBootPrefs(doc, loc = window.location) {
  const prefs = loadPrefs()
  applyReduceMotion(doc, prefs.reduce_motion)
  applyDensity(doc, prefs.density)
  if (prefs.startup_view === 'last' && !loc.hash) {
    let last = null
    try {
      last = localStorage.getItem('openremote-last-view')
    } catch {
      last = null
    }
    if (last) loc.hash = `#${last}`
  }
}

// Recently used workspace folders - one shared parse instead of a copy
// per view. Capped and deduped on write, most recent first.
export function loadRecentWorkspaces() {
  try {
    const raw = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? '[]')
    return Array.isArray(raw) ? raw.filter((p) => typeof p === 'string').slice(0, 8) : []
  } catch {
    return []
  }
}

export function pushRecentWorkspace(path) {
  const list = [path, ...loadRecentWorkspaces().filter((p) => p !== path)].slice(0, 8)
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(list))
  } catch {
    /* storage unavailable */
  }
  return list
}

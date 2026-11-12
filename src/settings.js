// Console-side preferences, persisted locally. Pure functions on the
// document/location-shaped objects the caller passes, so tests run
// without a DOM. The reducer-shaped key set is open: a new pref is a
// field here plus an apply line in restoreBootPrefs.

const PREFS_KEY = 'openremote-prefs'
const RECENTS_KEY = 'openremote-recent-workspaces'
// New chat presets: a list the person edits (saves, renames, deletes),
// which is why it gets its own key like the recents rather than a field in
// the prefs envelope. The prefs are read before the first paint and
// whitelisted field by field; a list of records has no business in that
// path, and one damaged preset must never take the boot with it.
const PRESETS_KEY = 'openremote-presets'
// When this console last looked at each chat: a map of chat id to the
// activity stamp it showed then. Its own key for the presets' reason - it is
// a record the person's looking writes, not a preference the boot applies,
// and one damaged entry must read as "no record" rather than take the first
// paint with it.
const CHAT_SEEN_KEY = 'openremote-chat-seen'
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
  // The superseded key is read under its own guard. This runs before the
  // first paint (main.jsx), so a storage that refuses to answer has to
  // leave the default in place rather than take the boot down with it.
  let migrated = false
  try {
    migrated = localStorage.getItem(OLD_REDUCE_KEY) === '1'
  } catch {
    migrated = false
  }
  const reduce =
    typeof raw.reduce_motion === 'boolean' ? raw.reduce_motion : migrated
  return {
    reduce_motion: reduce === true,
    density: DENSITIES.includes(raw.density) ? raw.density : 'comfortable',
    startup_view: raw.startup_view === 'last' ? 'last' : 'new',
    privacy_mode: raw.privacy_mode === true,
    // Moving a chat to another project is asked first until the dialog's
    // "Don't show this again" turns it off; Settings turns it back on.
    ask_move_chat: raw.ask_move_chat !== false,
  }
}

// Writes one preference over the stored set, and answers whether the write
// landed. A storage that refuses (a locked-down profile, a full disk) leaves
// the console showing the value it already has, so the answer is what tells a
// caller the difference between "kept" and "the row re-read the old value":
// switching a document attribute on a write nobody kept would leave the app
// and its own settings row disagreeing.
//
// The stored set is read here rather than through loadPrefs, because that one
// answers defaults for anything it cannot read: a profile that refuses the read
// but allows the write would have every other preference written back as its
// default, which is a silent reset of settings nobody touched. A set that is
// there and cannot be parsed is refused instead, and the caller says so.
export function savePrefs(partial) {
  try {
    const raw = localStorage.getItem(PREFS_KEY)
    let stored = {}
    if (raw == null) {
      // Nothing stored yet: the defaults plus the superseded reduce-motion key
      // are the best truth there is, and the first save is what writes them.
      stored = loadPrefs()
    } else {
      stored = JSON.parse(raw)
      if (!stored || typeof stored !== 'object' || Array.isArray(stored)) return false
    }
    localStorage.setItem(PREFS_KEY, JSON.stringify({ ...stored, ...partial }))
    return true
  } catch {
    return false
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

// Privacy mode blurs what would identify the user or their work on a
// recording or a stream - titles, projects, paths, device names, the
// account email, the transcript. Every element that carries such text
// wears the `pv` class; privacy.css keys the blur on this attribute.
export function applyPrivacyMode(doc, value) {
  if (value) doc.documentElement.setAttribute('data-privacy', '')
  else doc.documentElement.removeAttribute('data-privacy')
}

// Run once before first React paint: restores the persisted look and,
// when the user asked to reopen their last view, seeds the hash the
// router already knows how to validate. A garbage last-view is left
// for loadViewState's known-set to fall back from - never a crash.
export function restoreBootPrefs(doc, loc = window.location) {
  const prefs = loadPrefs()
  applyReduceMotion(doc, prefs.reduce_motion)
  applyDensity(doc, prefs.density)
  applyPrivacyMode(doc, prefs.privacy_mode)
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

// Whether the shell's hash-write effect may run yet. The seed above can
// be a chat id, which only the session list can validate - and that
// list is empty until the console's first poll beat - so the [sessions]
// reconcile (the hash's only reader) must consume the seed before the
// write effect replaceState's #new over it and stamps 'new' into the
// last-view record. Two things release the hold: a session beat having
// answered (any list the console set after mount, even an empty one -
// an answer is an answer), or the seed being consumed already (nothing
// was planted, or the user navigated, which supersedes it). Pure, so
// the hold's rule is tested without a DOM.
export function hashWriteReady({ sessionsLoaded = false, seedConsumed = false } = {}) {
  return sessionsLoaded || seedConsumed
}

/// Two spellings of one folder. A trailing separator is the same folder on
/// every platform, and a Windows path is case-insensitive besides. A POSIX
/// path is not, so case is folded only when either side looks like Windows -
/// otherwise `~/Work` and `~/work` would collapse into one entry and a
/// recent folder would disappear.
export function samePath(a, b) {
  const trim = (value) => String(value ?? '').replace(/[\\/]+$/, '')
  const left = trim(a)
  const right = trim(b)
  const windows = (value) => /^[a-zA-Z]:/.test(value) || value.includes('\\')
  if (windows(left) || windows(right)) return left.toLowerCase() === right.toLowerCase()
  return left === right
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
  const list = [path, ...loadRecentWorkspaces().filter((p) => !samePath(p, path))].slice(0, 8)
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(list))
  } catch {
    /* storage unavailable */
  }
  return list
}

// New chat presets, stored as the list the person sees. This layer only
// guarantees the shape of the container - an array of objects - and never
// reads inside a row: what a preset means is presets.js's rule, so a
// damaged row is dropped there rather than silently reinterpreted here.
export function loadStoredPresets() {
  try {
    const raw = JSON.parse(localStorage.getItem(PRESETS_KEY) ?? '[]')
    return Array.isArray(raw) ? raw.filter((row) => row && typeof row === 'object' && !Array.isArray(row)) : []
  } catch {
    return []
  }
}

export function saveStoredPresets(rows) {
  try {
    localStorage.setItem(PRESETS_KEY, JSON.stringify(Array.isArray(rows) ? rows : []))
  } catch {
    /* storage unavailable: the session keeps what it has in memory */
  }
}
// Which chats this console has looked at since, stored as the map itself.
// This layer guarantees only the container - a plain object - and never
// reads a stamp: what counts as moved, which entries are junk and how many
// are worth keeping is unread.js's rule, so a damaged entry is dropped
// there rather than silently reinterpreted here.
export function loadStoredChatSeen() {
  try {
    const raw = JSON.parse(localStorage.getItem(CHAT_SEEN_KEY) ?? '{}')
    return raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {}
  } catch {
    return {}
  }
}

export function saveStoredChatSeen(seen) {
  try {
    const map = seen && typeof seen === 'object' && !Array.isArray(seen) ? seen : {}
    localStorage.setItem(CHAT_SEEN_KEY, JSON.stringify(map))
  } catch {
    /* storage unavailable: the session keeps what it has in memory */
  }
}
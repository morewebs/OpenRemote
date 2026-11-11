// The built-in browser's tabs, downloads and find results, kept for the
// app's lifetime in a plain module (not the view's), so the view can come
// and go - and be edited in development - without losing them. The shell
// owns the pages; this keeps what the console knows about them and hears
// the shell's `browser:*` events.

import { hasTauri, isMobile } from './platform.js'
import { insertAfter, newTab, popupBelongs, putAway, restoredTabs, sessionOf, toDiscard, withRestored } from './browser-tabs.js'
import { bookmarkFor, inFolder, positionAt, stepPosition, withBookmark, withoutBookmark } from './browser-bookmarks.js'
import { HISTORY_CAP, recordVisit as pushVisit, visitsBefore } from './browser-history.js'
import { linkAbort, loadConnection } from './api/daemon.js'
import { browserSetting, getBrowserPrefs, loadBrowserPrefs, setSitePermission } from './browser-prefs.js'
import { rememberedAnswer } from './browser-chrome.js'
import { recordFrame, recording, startRecording, stopRecording, subscribeRecording, tabClosed } from './browser-record.js'

// A bookmark call waits this long for the daemon before it is treated as a
// daemon that did not answer. A write that never settles is the worst case for
// this screen: the row the person just kept would sit there as if it had been
// saved, so every call here has a budget and a timeout becomes the stale notice.
const BROWSER_CALL_TIMEOUT_MS = 8000

// Bookmarks follow the account; history stays on this device. The daemon keeps
// the bookmark rows on disk, because that is the store the sync note reads, so
// the console writes through its browser routes and reads back what it says.
// localStorage remains only as the first paint's cache: when the daemon
// answers, the daemon wins. History is the reading record of one person at one
// keyboard and nothing here syncs it.
const BOOKMARKS_KEY = 'openremote-browser-bookmarks'
const HISTORY_KEY = 'openremote-browser-history'

function loadBookmarks() {
  try {
    const raw = JSON.parse(localStorage.getItem(BOOKMARKS_KEY) ?? '{"marks":[]}')
    if (!Array.isArray(raw?.marks)) return []
    return raw.marks.filter((m) => m && typeof m.url === 'string' && m.url)
  } catch {
    return []
  }
}

function storeBookmarks(marks) {
  try {
    localStorage.setItem(BOOKMARKS_KEY, JSON.stringify({ v: 1, marks }))
  } catch {
    // No storage: the list still lives for this window.
  }
}

/// A call to the browser's own routes, on the daemon this console is connected
/// to. Throws the daemon's sentence when it refuses, so the caller can say it.
export async function browserCall(method, path, body = null) {
  const { url, token } = loadConnection()
  if (!url || !token) throw new Error('The console is not connected to its daemon yet.')
  const base = /^https?:\/\//.test(url) ? url : `http://${url}`
  const abort = linkAbort({ timeoutMs: BROWSER_CALL_TIMEOUT_MS })
  try {
    const response = await fetch(`${base.replace(/\/+$/, '')}${path}`, {
      method,
      headers: {
        ...(body ? { 'Content-Type': 'application/json' } : {}),
        Authorization: `Bearer ${token}`,
      },
      body: body ? JSON.stringify(body) : undefined,
      signal: abort.signal,
    })
    const text = await response.text()
    let answer = null
    try {
      answer = text ? JSON.parse(text) : null
    } catch {
      answer = null
    }
    if (!response.ok) {
      const failure = new Error(answer?.error ?? `HTTP ${response.status}`)
      // The status is kept because a 404 is not the same fact as silence: it
      // means this daemon does not know the route (an older build), and the
      // sentence the person reads should say that rather than "no answer".
      failure.status = response.status
      throw failure
    }
    return answer
  } catch (err) {
    if (abort.timedOut) throw new Error('The daemon did not answer in time.')
    throw err
  } finally {
    abort.detach()
  }
}

function requestId() {
  return `mark-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
}

// A change the daemon did not take goes back to the list as it was, and the
// bar says the list is what it last saw rather than showing a save that never
// happened. The reason is the daemon's, sorted into three facts a person can
// act on.
function noteFor(err) {
  const status = err?.status
  if (status === 404) return 'Bookmarks are the list this window last saw: this daemon does not keep them yet.'
  if (status) return `Bookmarks are the list this window last saw: ${err.message}`
  return 'Bookmarks are the list this window last saw: the daemon did not answer.'
}

function unsaved(err, fallback) {
  setState(() => ({ bookmarks: fallback, bookmarksStale: true, bookmarksNote: noteFor(err) }))
  storeBookmarks(fallback)
}

async function readBookmarks() {
  const answer = await browserCall('GET', '/browser/bookmarks')
  return Array.isArray(answer?.bookmarks) ? answer.bookmarks : []
}

// The daemon's rows replace the cache. Called on the first subscriber (the
// browser view mounting) and after a change it made.
export async function refreshBookmarks() {
  try {
    let marks = await readBookmarks()
    // The rows this window could already show, from before the daemon kept
    // them: an update must never empty a bar that was full. When the daemon
    // holds none, the cache is pushed once, and the daemon is the truth after.
    if (marks.length === 0) {
      const cached = loadBookmarks()
      if (cached.length > 0) {
        for (const mark of cached) {
          await browserCall('POST', '/browser/bookmarks', { request_id: requestId(), ...mark }).catch(() => {})
        }
        marks = await readBookmarks().catch(() => cached)
      }
    }
    setState(() => ({ bookmarks: marks, bookmarksStale: false, bookmarksNote: null }))
    storeBookmarks(marks)
    return true
  } catch (err) {
    // Keep what the person can see, and say it is the cache.
    setState(() => ({ bookmarksStale: true, bookmarksNote: noteFor(err) }))
    return false
  }
}

async function writeBookmark(mark, fallback) {
  try {
    await browserCall('POST', '/browser/bookmarks', { request_id: requestId(), ...mark })
    const marks = getState().bookmarks
    setState(() => ({ bookmarksStale: false, bookmarksNote: null }))
    storeBookmarks(marks)
  } catch (err) {
    unsaved(err, fallback)
  }
}

async function unwriteBookmark(id, fallback) {
  try {
    await browserCall('POST', `/browser/bookmarks/${encodeURIComponent(id)}/remove`, {
      request_id: requestId(),
    })
    const marks = getState().bookmarks
    setState(() => ({ bookmarksStale: false, bookmarksNote: null }))
    storeBookmarks(marks)
  } catch (err) {
    unsaved(err, fallback)
  }
}

function loadHistory() {
  try {
    const raw = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '{"visits":[]}')
    if (!Array.isArray(raw?.visits)) return []
    return raw.visits.filter((v) => v && typeof v.url === 'string' && v.url).slice(0, HISTORY_CAP)
  } catch {
    return []
  }
}

function storeHistory(visits) {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify({ v: 1, visits }))
  } catch {
    // No storage: the list still lives for this window.
  }
}

function newBookmarkId() {
  return `bm-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
}

let started = false
let state = (() => {
  // Stamped as shown from the start, so it is not the first put away.
  const first = newTab({ seen: Date.now() })
  return {
    tabs: [first],
    active: first.key,
    closed: [],
    downloads: [],
    finds: {},
    // Sites' requests waiting for the user's answer (the permission bar):
    // { tab, id, origin, kind, label, private }.
    prompts: [],
    // The page showing something full screen (a video's button), by the
    // shell's id: the view gives it the whole window.
    fullscreen: null,
    features: null,
    bookmarks: loadBookmarks(),
    // Whether the list is the daemon's, or only this window's last cache, and
    // the sentence that says which fact made it a cache.
    bookmarksStale: false,
    bookmarksNote: null,
    history: loadHistory(),
  }
})()
const listeners = new Set()
const keyHandlers = new Set()
// What the shell said about a page before its tab knew its id.
const early = new Map()

// Which page the user was on, by the shell's own id, so a console reload
// brings back the tab that was open instead of the first one. sessionStorage
// rather than localStorage: a fresh launch should still start on the first
// tab, and only a reload of this window is a continuation.
const ACTIVE_KEY = 'openremote-browser-active'
let rememberedActive = null

function rememberActive() {
  const tab = state.tabs.find((t) => t.key === state.active)
  const native = tab?.native ?? null
  if (!native || native === rememberedActive) return
  rememberedActive = native
  try {
    sessionStorage.setItem(ACTIVE_KEY, native)
  } catch {
    // No storage: the first tab is still a fine place to land.
  }
}

function wasActive() {
  try {
    return sessionStorage.getItem(ACTIVE_KEY)
  } catch {
    return null
  }
}

// Where a reload lands: the page the user was on if it is still open, the
// first tab otherwise (that tab was closed while the console was loading,
// or this window never had one).
export function restoredActive(tabs, remembered) {
  return tabs.find((t) => t.native === remembered)?.key ?? tabs[0]?.key
}

export function getState() {
  return state
}

export function setState(change) {
  const before = state
  state = { ...state, ...change(state) }
  // Least recently shown is put away first: `seen` is when a tab was last
  // on screen, so both the tab being left and the one shown are stamped -
  // a tab the user sat in for an hour ranks by when they left it, not by
  // when they entered it.
  if (state.active !== before.active) {
    const now = Date.now()
    state = {
      ...state,
      tabs: state.tabs.map((t) => (t.key === state.active || t.key === before.active ? { ...t, seen: now } : t)),
    }
  }
  rememberActive()
  for (const fn of listeners) fn()
  if (state.tabs !== before.tabs || state.active !== before.active) scheduleSave()
}

export function subscribe(fn) {
  listeners.add(fn)
  // The first surface to read the bookmarks asks the daemon for the rows it
  // keeps: the cache answered the first paint, this makes it true.
  if (!started) {
    started = true
    void refreshBookmarks()
  }
  return () => listeners.delete(fn)
}

export function patchTab(key, change) {
  setState((s) => ({ tabs: s.tabs.map((t) => (t.key === key ? { ...t, ...change } : t)) }))
}

// ---- bookmarks (this device only) ----

// Adds the page to a folder, or updates the bookmark that already holds the
// address (one address is one row on the bar). Returns the saved bookmark.
export function saveBookmark({ url, title = '', folder = 'bar', id = '' } = {}) {
  if (!url) return null
  const before = getState().bookmarks
  const clean = String(title ?? '').trim()
  const to = folder === 'other' ? 'other' : 'bar'
  const existing = bookmarkFor(before, url)
  const marks = existing
    ? before.map((m) => (m.id === existing.id ? { ...m, title: clean || m.title, folder: to } : m))
    : withBookmark(before, {
        id: id || newBookmarkId(),
        url,
        title: clean,
        folder: to,
        position: positionAt(inFolder(before, to), Number.MAX_SAFE_INTEGER),
      })
  setState(() => ({ bookmarks: marks }))
  storeBookmarks(marks)
  // The daemon keeps the row a note can carry; a refusal puts this list back.
  void writeBookmark(marks.find((m) => m.url === url), before)
  return marks.find((m) => m.url === url) ?? null
}

export function updateBookmark(id, change = {}) {
  const before = getState().bookmarks
  const next = before.map((m) =>
    m.id === id
      ? {
          ...m,
          ...change,
          title: String(change.title ?? m.title ?? '').trim(),
          folder: change.folder === 'other' ? 'other' : change.folder === 'bar' ? 'bar' : m.folder,
        }
      : m,
  )
  setState(() => ({ bookmarks: next }))
  storeBookmarks(next)
  const written = next.find((m) => m.id === id)
  if (written) void writeBookmark(written, before)
}

export function removeBookmark(id) {
  const before = getState().bookmarks
  const next = withoutBookmark(before, id)
  setState(() => ({ bookmarks: next }))
  storeBookmarks(next)
  void unwriteBookmark(id, before)
}

// One step up or down the bookmark's own folder, stopped at the ends.
export function moveBookmark(id, step) {
  const marks = getState().bookmarks
  const mark = marks.find((m) => m.id === id)
  if (!mark) return
  const position = stepPosition(marks, mark, step)
  if (position == null) return
  updateBookmark(id, { position })
}

export async function call(command, args) {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke(command, args).catch(() => null)
}

// Browser keys pressed while a page had focus, for whoever handles them.
export function onKey(fn) {
  keyHandlers.add(fn)
  return () => keyHandlers.delete(fn)
}

// Give a console tab its page, with anything the page already said.
export function adopt(key, native) {
  const said = early.get(native) ?? {}
  early.delete(native)
  clearTimeout(orphans.get(native))
  orphans.delete(native)
  patchTab(key, { native, ...said })
  enforceMemory()
}

/// Past ten open pages, the least recently shown are put away to entries
/// (browser-tabs.js `toDiscard`): their pages close in the shell, and each
/// opens again when it is next shown.
export function enforceMemory() {
  const doomed = toDiscard(state.tabs, state.active)
  if (!doomed.length) return
  const keys = new Set(doomed.map((t) => t.key))
  const natives = new Set(doomed.map((t) => t.native))
  for (const t of doomed) closePage(t.native)
  setState((s) => ({
    tabs: s.tabs.map((t) => (keys.has(t.key) ? putAway(t) : t)),
    prompts: s.prompts.filter((p) => !natives.has(p.tab)),
  }))
}

function fieldsOf(p) {
  return {
    url: p.url,
    title: p.title,
    loading: p.loading,
    favicon: p.favicon,
    canBack: p.can_back,
    canForward: p.can_forward,
    // The shell says whether the tab is private; the console needs it to keep
    // a private page out of history (BrowserView's visit effect) and to say so
    // in the strip. Absent means normal, which is what the shell sends for one.
    private: p.private === true,
    // A private tab the user handed to agents (its site menu's toggle).
    shared: p.shared === true,
    // Which chat's agent holds the tab ({ kind: 'agent' | 'paused', chat,
    // title }), or null: the strip's badge and the take-over bar.
    controller: p.controller ?? null,
  }
}

// ---- site permissions ----

// How long the shell holds a request before it turns it down (win.rs).
const PROMPT_MS = 60_000

export function dropPrompt(tab, id) {
  setState((s) => ({ prompts: s.prompts.filter((p) => !(p.tab === tab && p.id === id)) }))
}

// Every request a closed page left: its bar goes with the tab.
export function dropPromptsFor(native) {
  if (!native) return
  setState((s) => ({ prompts: s.prompts.filter((p) => p.tab !== native) }))
}

/// The user's answer to a site's request, remembered for the site when they
/// asked for that (never for a private tab). Resolves once the answer is
/// kept; a remembering the daemon refused rejects, so the bar's owner can
/// say why (the page still got its answer).
export function answerPrompt(prompt, allow, remember) {
  call('browser_permission', { tab: prompt.tab, id: prompt.id, allow })
  dropPrompt(prompt.tab, prompt.id)
  if (!remember || prompt.private) return Promise.resolve()
  return setSitePermission(browserCall, prompt.origin, prompt.kind, allow ? 'allow' : 'deny')
}

/// A site asked (`browser:permission`): a remembered answer is given at
/// once; otherwise the bar asks. A private tab always asks.
export async function onPermission(p) {
  // Private unless known otherwise: a request can arrive before the console
  // tab has adopted its page, so the tab list alone can't say. The shell's
  // own word about the page (kept in `early` until adopt) is used then, and
  // a page nobody has described yet is treated as private - it always asks
  // and nothing it answers is remembered.
  const tab = getState().tabs.find((t) => t.native === p.tab)
  const said = early.get(p.tab)
  const priv = tab ? Boolean(tab.private) : said ? said.private !== false : true
  if (!priv) {
    if (!getBrowserPrefs().loaded) await loadBrowserPrefs(browserCall)
    const kept = rememberedAnswer(getBrowserPrefs().permissions, p)
    if (kept) {
      call('browser_permission', { tab: p.tab, id: p.id, allow: kept === 'allow' })
      return
    }
  }
  setState((s) => ({
    prompts: [...s.prompts.filter((q) => !(q.tab === p.tab && q.id === p.id)), { ...p, private: priv }],
  }))
  // The shell turns an unanswered request down after a minute; its bar
  // goes then too.
  const timer = setTimeout(() => dropPrompt(p.tab, p.id), PROMPT_MS)
  // (Node's tests: a waiting bar must not hold the process open.)
  timer?.unref?.()
}

// ---- full screen ----

/// A page went full screen or came back (`browser:fullscreen`); the shell
/// has already made the window follow.
export function onFullscreen({ tab, on } = {}) {
  setState((s) => ({ fullscreen: on ? tab : s.fullscreen === tab ? null : s.fullscreen }))
}

// ---- the session: brought back once per run, saved as it changes ----

// Per run of the app (it survives a console reload, not a restart): the
// last session comes back once.
const RESTORED_KEY = 'openremote-browser-restored'
const SAVE_MS = 1000
const RETRY_MS = 5000
const RETRIES = 12

// Nothing is saved until the last session has been brought back (or passed
// over), so a fresh start never writes over it.
let restoring = true
let lastSaved = ''
let saveTimer = 0
let sessionStarted = null
let retries = 0
// The daemon's browser routes (browser-store's own `browserCall`; a test
// hands in its own).
let daemon = browserCall

function restoredThisRun() {
  try {
    return sessionStorage.getItem(RESTORED_KEY) === '1'
  } catch {
    return false
  }
}

function markRestored() {
  try {
    sessionStorage.setItem(RESTORED_KEY, '1')
  } catch {
    // No storage: a console reload would restore again.
  }
}

/// Bring the last session back (once per run, when "On startup" says
/// continue) and start saving the tabs. Called as the browser view mounts;
/// a daemon that does not answer yet is asked again a little later.
export function startSession(caller = browserCall) {
  daemon = caller
  sessionStarted ??= (async () => {
    await wired
    const prefs = await loadBrowserPrefs(daemon)
    if (!prefs.loaded) {
      sessionStarted = null
      if (retries < RETRIES) {
        retries += 1
        const timer = setTimeout(() => startSession(caller), RETRY_MS)
        timer?.unref?.()
      }
      return
    }
    const first = !restoredThisRun()
    markRestored()
    if (first && browserSetting(prefs.settings, 'startup') === 'continue') {
      const restored = restoredTabs(prefs.session)
      setState((s) => withRestored(s.tabs, s.active, restored))
    }
    lastSaved = JSON.stringify(prefs.session ?? null)
    restoring = false
    scheduleSave()
  })()
  return sessionStarted
}

function scheduleSave() {
  if (restoring) return
  const session = sessionOf(state.tabs, state.active)
  const json = JSON.stringify(session)
  if (json === lastSaved) return
  clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    lastSaved = json
    // A save that did not land is tried again at the next change.
    Promise.resolve(daemon('POST', '/browser/session', session)).catch(() => {
      if (lastSaved === json) lastSaved = ''
    })
  }, SAVE_MS)
  saveTimer?.unref?.()
}

// ---- the shell's news about a tab ----

// A page that no console tab claims within this long (an agent's whose hold
// was marked late) joins the strip in the background, if the shell still
// has it.
const ORPHAN_MS = 8000
const orphans = new Map()
// Pages the console closed: a late event about one never brings it back.
const gone = new Set()
// A page the app asked to show before its tab was in the strip (an agent's
// tab beside its chat), for a moment.
const WANT_MS = 10_000
let wanted = null

/// Close a page the console is done with (its tab closed, or put away).
export function closePage(native) {
  if (!native) return
  gone.add(native)
  early.delete(native)
  clearTimeout(orphans.get(native))
  orphans.delete(native)
  // A recorded tab closing ends its recording, saved as far as it went.
  tabClosed(native)
  call('browser_close', { tab: native })
}

// ---- recording ----

/// Record a tab's page (browser-record.js): one at a time, 90 seconds at
/// most, saved to Downloads, where the downloads menu lists it. `say` hears
/// the one sentence about how it ended. False when nothing started.
export function recordTab(native, say = () => {}) {
  return startRecording(native, call, (result) => {
    if (result.error) {
      say(result.error)
      return
    }
    setState((s) => ({
      downloads: [{ id: `rec-${Date.now()}`, url: '', path: result.path, state: 'done' }, ...s.downloads].slice(0, 50),
    }))
    say(`Recording saved to Downloads: ${result.name}`)
  })
}

export { recording, stopRecording, subscribeRecording }

/// Show the console tab holding a page of the shell's ("tab-3"). When that
/// tab isn't in the strip yet, the wish waits a moment for it.
export function focusTab(native) {
  const tab = state.tabs.find((t) => t.native === native)
  if (!tab) {
    wanted = { native, at: Date.now() }
    return false
  }
  wanted = null
  if (state.active !== tab.key) setState(() => ({ active: tab.key }))
  return true
}

function grantWish() {
  if (!wanted) return
  if (Date.now() - wanted.at > WANT_MS) {
    wanted = null
    return
  }
  focusTab(wanted.native)
}

/// What the shell says about a tab (`browser:tab`).
export function onTab(p) {
  if (!p?.id || gone.has(p.id)) return
  // A popup whose opener's tab this console does not have: no tab will
  // ever place it, so it is closed instead of staying in the shell's tab
  // list with no console tab, to come back as a stray tab on the next
  // console load. The console's own opens have no opener and are adopted
  // by the tab that asked for them.
  const tabs = getState().tabs
  if (p.opener && !popupBelongs(tabs, p.opener) && !tabs.some((t) => t.native === p.id)) {
    closePage(p.id)
    return
  }
  let joined = null
  setState((s) => {
    const fields = fieldsOf(p)
    if (s.tabs.some((t) => t.native === p.id)) {
      return { tabs: s.tabs.map((t) => (t.native === p.id ? { ...t, ...fields } : t)) }
    }
    const opener = s.tabs.find((t) => t.native && t.native === p.opener)
    if (opener) {
      // A page opened a window: a new tab beside its opener, shown when the
      // user's own page opened it (an agent's popup never takes the view).
      const show = opener.key === s.active && !p.controller
      joined = { show }
      const tab = newTab({ native: p.id, ...fields })
      return { tabs: insertAfter(s.tabs, tab, opener.key), active: show ? tab.key : s.active }
    }
    if (p.controller) {
      // A tab a chat's agent opened: in the strip, the user's view unmoved.
      const said = early.get(p.id)
      early.delete(p.id)
      clearTimeout(orphans.get(p.id))
      orphans.delete(p.id)
      joined = { show: false }
      return { tabs: [...s.tabs, newTab({ native: p.id, ...said, ...fields })] }
    }
    // A tab still opening: a fast page can finish loading before the
    // console hears which tab it is. Kept until it does (adopt).
    early.set(p.id, { ...early.get(p.id), ...fields })
    if (!orphans.has(p.id)) {
      const timer = setTimeout(() => claimOrphan(p.id), ORPHAN_MS)
      timer?.unref?.()
      orphans.set(p.id, timer)
    }
    return {}
  })
  if (joined) {
    // One that isn't shown stays out of sight until the user picks it.
    if (!joined.show) call('browser_hide', { tab: p.id })
    grantWish()
  }
  enforceMemory()
}

// A page nobody claimed joins the strip in the background, if the shell
// still has it.
async function claimOrphan(id) {
  orphans.delete(id)
  if (!early.has(id) || gone.has(id) || state.tabs.some((t) => t.native === id)) return
  const open = await call('browser_tabs')
  const fields = early.get(id)
  if (!fields || !open?.some((t) => t.id === id) || state.tabs.some((t) => t.native === id)) return
  early.delete(id)
  setState((s) => ({ tabs: [...s.tabs, newTab({ native: id, ...fields })] }))
  call('browser_hide', { tab: id })
  grantWish()
  enforceMemory()
}

async function wire() {
  const { listen } = await import('@tauri-apps/api/event')
  listen('browser:tab', ({ payload }) => onTab(payload))
  listen('browser:download', ({ payload: d }) => {
    setState((s) => {
      const known = s.downloads.find((x) => x.id === d.id)
      const rest = s.downloads.filter((x) => x.id !== d.id)
      return { downloads: [{ ...known, ...d, path: d.path ?? known?.path }, ...rest].slice(0, 50) }
    })
  })
  listen('browser:find', ({ payload: f }) => {
    setState((s) => ({ finds: { ...s.finds, [f.tab]: { count: f.count, active: f.active } } }))
  })
  listen('browser:key', ({ payload }) => {
    for (const fn of keyHandlers) fn(payload.chord)
  })
  listen('browser:permission', ({ payload }) => onPermission(payload))
  listen('browser:fullscreen', ({ payload }) => onFullscreen(payload))
  // A recorded tab's pictures (browser_screencast), for browser-record.js.
  listen('browser:frame', ({ payload }) => recordFrame(payload))
  // What this build's shell can do with a tab: find in page, the DevTools
  // bridge (element picking), device view and forgetting a site are written
  // against Windows' engine, and a control the shell would refuse must not
  // be drawn as if it worked. Asked once, before the first page. An ask that
  // never landed is stored as "answers none", not left as "no answer": unknown
  // must not read as yes downstream, and the view says so once rather than
  // drawing controls that would do nothing.
  const features = await call('browser_features')
  setState(() => ({ features: features ?? {} }))
  // Pages the shell already holds, taken back hidden until the view shows
  // one rather than left on screen, orphaned. After a console reload (a
  // development reload, a crash of the console's page) they are the tabs as
  // they were. Before the browser's first open in a run they are tabs agents
  // opened: they join behind the new tab page, which the last session may
  // yet replace (01aee56) - read as a reload, the restore was skipped and
  // the session then saved over.
  const open = await call('browser_tabs')
  if (open?.length) {
    for (const t of open) await call('browser_hide', { tab: t.id })
    const tabs = open.map((t) => newTab({ native: t.id, ...fieldsOf(t) }))
    if (restoredThisRun()) setState(() => ({ tabs, active: restoredActive(tabs, wasActive()) }))
    else setState((s) => ({ tabs: [...s.tabs, ...tabs] }))
  }
}

const wired = hasTauri && !isMobile ? wire() : Promise.resolve()

// The browser's view is the only thing that can reopen a tab (it builds the
// page and places it), so it registers that one action while it is mounted and
// the palette's row asks for it. With no registrant the row is not drawn at
// all: a command whose only effect is nothing is worse than a missing row.
let reopenHandler = null

export function registerReopen(fn) {
  reopenHandler = fn
  return () => {
    if (reopenHandler === fn) reopenHandler = null
  }
}

/// Whether reopening would do anything right now: a view that can do it, and
/// at least one closed tab to go back to.
export function reopenAvailable() {
  return Boolean(reopenHandler) && getState().closed.length > 0
}

/// Whether a browser view is on screen right now. The palette's Browser group
/// asks this rather than `reopenAvailable` above: pages are worth listing
/// whether or not a closed one exists, and a tab nobody can see is not a place
/// a command can send a person. Both facts come from the one registration the
/// view makes while it is mounted.
export function browserMounted() {
  return Boolean(reopenHandler)
}

export function reopenClosed() {
  reopenHandler?.()
}

// ---- history (this device only) ----

// Called as the pane shows a page. The same address twice in a row is one
// visit, so a title landing a moment later updates the row rather than adding
// a second one; the pure rule owns both facts.
export function recordVisit({ url, title = '', at = Date.now() } = {}) {
  const before = getState().history
  const visits = pushVisit(before, { url, title, at })
  if (visits === before) return
  setState(() => ({ history: visits }))
  storeHistory(visits)
}

// The daemon keeps a reading record of its own (the shell's pages, never a
// private one), and the new tab page's most-visited shortcuts are ranked
// from it: clearing history clears it there too, over the same range, or a
// cleared page would still be a shortcut. This window's record goes first,
// whatever the daemon says; a refusal is then thrown for the caller to say.
async function clearDaemonHistory(since, caller) {
  await caller('POST', '/browser/history/clear', since == null ? {} : { since })
}

export async function clearHistory(caller = browserCall) {
  setState(() => ({ history: [] }))
  storeHistory([])
  await clearDaemonHistory(null, caller)
}

// "Clear browsing data" over a time range: the visits from `since` on go,
// older ones stay (null clears it all).
export async function clearHistorySince(since, caller = browserCall) {
  const visits = visitsBefore(getState().history, since)
  setState(() => ({ history: visits }))
  storeHistory(visits)
  await clearDaemonHistory(since, caller)
}

// Editing this module in development reloads the console (the restore
// above then takes the pages back) instead of starting a second tab list.
if (import.meta.hot) import.meta.hot.decline()

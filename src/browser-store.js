// The built-in browser's tabs, downloads and find results, kept for the
// app's lifetime in a plain module (not the view's), so the view can come
// and go - and be edited in development - without losing them. The shell
// owns the pages; this keeps what the console knows about them and hears
// the shell's `browser:*` events.

import { hasTauri, isMobile } from './platform.js'
import { insertAfter, newTab, popupBelongs } from './browser-tabs.js'
import { bookmarkFor, inFolder, positionAt, stepPosition, withBookmark, withoutBookmark } from './browser-bookmarks.js'
import { HISTORY_CAP, recordVisit as pushVisit, visitsBefore } from './browser-history.js'
import { linkAbort, loadConnection } from './api/daemon.js'
import { getBrowserPrefs, loadBrowserPrefs, setSitePermission } from './browser-prefs.js'
import { rememberedAnswer } from './browser-chrome.js'

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
  const first = newTab()
  return {
    tabs: [first],
    active: first.key,
    closed: [],
    downloads: [],
    finds: {},
    // Sites' requests waiting for the user's answer (the permission bar):
    // { tab, id, origin, kind, label, private }.
    prompts: [],
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
  state = { ...state, ...change(state) }
  rememberActive()
  for (const fn of listeners) fn()
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
  patchTab(key, { native, ...said })
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
  const priv = getState().tabs.some((t) => t.native === p.tab && t.private)
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

async function wire() {
  const { listen } = await import('@tauri-apps/api/event')
  listen('browser:tab', ({ payload: p }) => {
    // A popup whose opener's tab this console does not have: no tab will
    // ever place it, so it is closed instead of staying in the shell's tab
    // list with no console tab, to come back as a stray tab on the next
    // console load. The console's own opens have no opener and are adopted
    // by the tab that asked for them.
    const tabs = getState().tabs
    if (p.opener && !popupBelongs(tabs, p.opener) && !tabs.some((t) => t.native === p.id)) {
      call('browser_close', { tab: p.id })
      return
    }
    setState((s) => {
      const fields = fieldsOf(p)
      if (s.tabs.some((t) => t.native === p.id)) {
        return { tabs: s.tabs.map((t) => (t.native === p.id ? { ...t, ...fields } : t)) }
      }
      // A page opened a window: a new tab beside its opener, shown.
      const opener = s.tabs.find((t) => t.native && t.native === p.opener)
      if (!opener) {
        // A tab still opening: a fast page can finish loading before the
        // console hears which tab it is. Kept until it does (adopt).
        early.set(p.id, fields)
        return {}
      }
      const tab = newTab({ native: p.id, ...fields })
      return { tabs: insertAfter(s.tabs, tab, opener.key), active: tab.key }
    })
  })
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
  // What this build's shell can do with a tab: find in page, the DevTools
  // bridge (element picking), device view and forgetting a site are written
  // against Windows' engine, and a control the shell would refuse must not
  // be drawn as if it worked. Asked once, before the first page. An ask that
  // never landed is stored as "answers none", not left as "no answer": unknown
  // must not read as yes downstream, and the view says so once rather than
  // drawing controls that would do nothing.
  const features = await call('browser_features')
  setState(() => ({ features: features ?? {} }))
  // The console reloaded while the shell kept its pages (a development
  // reload, a crash of the console's page): take them back, hidden until
  // the view shows one, rather than leave them on screen, orphaned.
  const open = await call('browser_tabs')
  if (open?.length) {
    for (const t of open) await call('browser_hide', { tab: t.id })
    const tabs = open.map((t) => newTab({ native: t.id, ...fieldsOf(t) }))
    setState(() => ({ tabs, active: restoredActive(tabs, wasActive()) }))
  }
}

if (hasTauri && !isMobile) wire()

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

export function clearHistory() {
  setState(() => ({ history: [] }))
  storeHistory([])
}

// "Clear browsing data" over a time range: the visits from `since` on go,
// older ones stay (null clears it all).
export function clearHistorySince(since) {
  const visits = visitsBefore(getState().history, since)
  setState(() => ({ history: visits }))
  storeHistory(visits)
}

// Editing this module in development reloads the console (the restore
// above then takes the pages back) instead of starting a second tab list.
if (import.meta.hot) import.meta.hot.decline()

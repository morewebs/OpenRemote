// What the browser's address bar does with what was typed: an address to
// open, or a search. Pure: tested without a DOM.

export const SEARCH_ENGINES = {
  google: { name: 'Google', url: 'https://www.google.com/search?q=' },
  duckduckgo: { name: 'DuckDuckGo', url: 'https://duckduckgo.com/?q=' },
  bing: { name: 'Bing', url: 'https://www.bing.com/search?q=' },
  brave: { name: 'Brave Search', url: 'https://search.brave.com/search?q=' },
  startpage: { name: 'Startpage', url: 'https://www.startpage.com/do/search?q=' },
}

export function searchUrl(text, engine = 'google') {
  const base = (SEARCH_ENGINES[engine] ?? SEARCH_ENGINES.google).url
  return base + encodeURIComponent(text)
}

const LOCAL = /^(localhost|127(?:\.\d{1,3}){3}|\[::1\]|\d{1,3}(?:\.\d{1,3}){3})(:\d{1,5})?([/?#].*)?$/i
const DOMAIN = /^[^\s/?#:]+\.[a-z][a-z0-9-]*\.?(:\d{1,5})?([/?#]\S*)?$/i
const SCHEME = /^(https?|file):\/\/\S+$/i

// The address to open for a line typed in the address bar, or null when
// there is nothing to open. A script address never runs from here.
export function omnibox(input, engine = 'google') {
  const text = (input ?? '').trim()
  if (!text) return null
  if (/^javascript:/i.test(text)) return null
  if (SCHEME.test(text) || /^about:(blank)$/i.test(text)) return text
  // A local address is the user's own dev server: plain http.
  if (LOCAL.test(text)) return `http://${text}`
  if (DOMAIN.test(text)) return `https://${text}`
  return searchUrl(text, engine)
}

// The address a typed line names, or null when the line is words (a search)
// or nothing: what a new tab page shortcut can be saved as.
export function addressFor(input) {
  const text = (input ?? '').trim()
  if (!text || /^javascript:/i.test(text)) return null
  if (SCHEME.test(text) || LOCAL.test(text) || DOMAIN.test(text)) return omnibox(text)
  return null
}

// An address as people type it: no scheme, no leading www.
export function bareAddress(url) {
  return String(url ?? '')
    .toLowerCase()
    .replace(/^[a-z][a-z0-9+.-]*:\/\//, '')
    .replace(/^www\./, '')
}

// What the address bar shows for a page: the address as it is, except a
// search shows its words (Chrome shows the address; we keep that, plain).
export function shownAddress(url) {
  if (!url || url === 'about:blank') return ''
  return url
}

// Whether a page arrived over a secure connection, for the lock.
export function security(url) {
  if (!url) return 'none'
  if (url.startsWith('https://')) return 'secure'
  if (/^http:\/\/(localhost|127\.|\[::1\])/i.test(url)) return 'local'
  if (url.startsWith('http://')) return 'insecure'
  return 'none'
}

// ---- what the address bar suggests while a prefix is typed ----
//
// The two things this device kept and read: bookmarks and history. A row is a
// candidate address with the title to draw, whether it is kept, and the score
// it earned. Pure, so the ranking is pinned without a DOM: an exact prefix on
// a title beats the same letters inside a title, anything kept beats a plain
// history hit of the same quality, and among equals the most recent visit
// wins. One address is one row (the kept one, if both know it), and the list
// is short on purpose: the field is for going somewhere, not for browsing.
export const SUGGEST_LIMIT = 6

function bareUrl(url) {
  return String(url ?? '').replace(/^https?:\/\//i, '').toLowerCase()
}

function quality(entry, needle) {
  const title = String(entry?.title ?? '').toLowerCase()
  const bare = bareUrl(entry?.url)
  if (title.startsWith(needle)) return 0
  if (bare.startsWith(needle)) return 1
  if (title.includes(needle)) return 2
  if (bare.includes(needle)) return 3
  return null
}

export function suggest({ typed = '', marks = [], visits = [], limit = SUGGEST_LIMIT } = {}) {
  const needle = String(typed ?? '').trim().toLowerCase()
  if (!needle) return []
  const seen = new Map()
  const add = (entry, kept) => {
    const score = quality(entry, needle)
    if (score == null) return
    const found = seen.get(entry.url)
    // A kept address outranks a plain visit of the same quality, but a better
    // match elsewhere in the list still wins: the flag only breaks ties.
    if (found && (found.kept || !kept) && found.score <= score) {
      if (kept) found.kept = true
      return
    }
    seen.set(entry.url, {
      url: entry.url,
      title: kept ? String(entry.title ?? '') : String(entry.title ?? ''),
      kept: kept || Boolean(found?.kept),
      score,
      at: Number(entry.at ?? entry.visitedAt ?? 0) || 0,
    })
  }
  for (const mark of marks ?? []) if (mark?.url) add(mark, true)
  for (const visit of visits ?? []) if (visit?.url) add(visit, false)
  return [...seen.values()]
    .sort((a, b) => a.score - b.score || (a.kept ? 0 : 1) - (b.kept ? 0 : 1) || b.at - a.at || a.url.localeCompare(b.url))
    .slice(0, Math.max(0, limit))
}
// ---- inline completion ----
//
// Chrome completes a typed beginning in the field itself: "git" shows
// "github.com" with "hub.com" selected, so the next key replaces it, Tab or
// Right takes it, and Enter opens it. The completion is the site, or the
// whole address once the typing reaches past the site. Words (a search) and
// a typed scheme are never completed.

/** What `typed` completes to from one suggested address, or null. */
export function completion(typed, url) {
  const text = String(typed ?? '')
  if (!text || /\s/.test(text) || text.includes('://')) return null
  const lower = text.toLowerCase().replace(/^www\./, '')
  const bare = bareAddress(url).replace(/\/$/, '')
  if (!lower || !bare.startsWith(lower)) return null
  const whole = lower.includes('/') ? bare : bare.split('/')[0]
  if (whole.length <= lower.length) return null
  return text + whole.slice(lower.length)
}

/** The first completion the suggestion rows offer for `typed`, or null. */
export function inlineFor(typed, rows) {
  for (const row of rows ?? []) {
    const done = completion(typed, row?.url)
    if (done) return done
  }
  return null
}

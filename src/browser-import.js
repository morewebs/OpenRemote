// Importing bookmarks and history from another browser on this computer, as
// Settings > Browser shows it. The daemon does the reading (`/browser/import`):
// it finds the other browsers' profiles in their own folders, or reads a
// folder the user picked, opens only their bookmarks and history (from private
// copies, read-only), and keeps what it brings in. Bookmarks join Other
// bookmarks and follow the account like any other; history stays on this
// computer. This module only reads the daemon's answers and words them.

/// The profiles the daemon found, as the console reads them: anything it did
/// not send is left out.
export function readSources(raw) {
  const list = Array.isArray(raw?.sources) ? raw.sources : []
  return list
    .filter((s) => s && typeof s.id === 'string' && typeof s.browser === 'string')
    .map((s) => ({
      id: s.id,
      browser: s.browser,
      profile: typeof s.profile === 'string' ? s.profile : '',
      bookmarks: s.bookmarks === true,
      history: s.history === true,
    }))
    .filter((s) => s.bookmarks || s.history)
}

/// One profile as a row names it: the browser, then which profile when the
/// browser has a name for it.
export function sourceLabel(source) {
  const profile = source.profile && source.profile !== source.browser ? source.profile : ''
  return profile ? `${source.browser} (${profile})` : source.browser
}

const count = (n, one, many) => `${n} ${n === 1 ? one : many}`

/// The sentence after an import: what came in, and what was left out and why.
export function importLine(done) {
  const d = done && typeof done === 'object' ? done : {}
  const num = (v) => (Number.isFinite(v) && v > 0 ? v : 0)
  const marks = num(d.bookmarks)
  const visits = num(d.visits)
  const from = typeof d.browser === 'string' && d.browser ? ` from ${d.browser}` : ''
  const parts = []
  if (marks || visits) {
    const what = []
    if (marks) what.push(`${count(marks, 'bookmark', 'bookmarks')} (in Other bookmarks)`)
    if (visits) what.push(count(visits, 'page', 'pages') + ' of history')
    parts.push(`Imported ${what.join(' and ')}${from}.`)
  } else {
    parts.push(`Nothing new to import${from}.`)
  }
  if (num(d.already)) parts.push(`${count(num(d.already), 'bookmark was', 'bookmarks were')} already here.`)
  if (num(d.over_cap)) {
    parts.push(`${count(num(d.over_cap), 'bookmark was', 'bookmarks were')} left out: one import adds at most 2,000, so your other devices are not flooded.`)
  }
  if (num(d.refused)) {
    parts.push(`${count(num(d.refused), 'entry was', 'entries were')} skipped for not being a web page (scripts, local files, inline data).`)
  }
  return parts.join(' ')
}

/// The profiles found on this computer, through `call(method, path, body)`
/// (browser-store's `browserCall`). A failure throws for errorCopy.
export async function loadSources(call) {
  return readSources(await call('GET', '/browser/import'))
}

/// Run one import: a listed profile by its id, or a folder the user picked.
export async function runImport(call, { source = null, path = null, bookmarks = true, history = true } = {}) {
  const body = { bookmarks, history }
  if (source) body.source = source
  else if (path) body.path = path
  return call('POST', '/browser/import', body)
}

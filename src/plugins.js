// The words the Plugins page and its Install dialog use for a plugin -
// one place, so the approval screen and the installed row never disagree.
// Pure: tested without a DOM.

export const RUNTIME_LABEL = {
  process: 'Program',
  js: 'JavaScript, sandboxed',
  wasm: 'WebAssembly, sandboxed',
  content: 'Claude plugin',
}

// Each power in plain words, as the user approves it.
export const POWER_LABEL = {
  run_programs: 'Run programs on this computer',
  read_chats: 'Read chats and project files',
  agent_tools: 'Give agents tools, and start, steer and stop chats',
  network: 'Reach the internet',
  microphone: 'Use the microphone',
}

export const POWER_SHORT = {
  run_programs: 'Runs programs',
  read_chats: 'Reads chats',
  agent_tools: 'Agent tools',
  network: 'Network',
  microphone: 'Microphone',
}

// Catalog ids are often "browser-use". The card should read "Browser Use".
export function pluginTitle(name) {
  const text = String(name ?? '').trim()
  if (!text) return ''
  if (/[A-Z]/.test(text) && !/[-_]/.test(text)) return text
  return text
    .replace(/[-_]+/g, ' ')
    .replace(/\b[a-z]/g, (letter) => letter.toUpperCase())
}

export const CLAUDE_PART_LABEL = {
  skills: 'skills',
  agents: 'agents',
  commands: 'commands',
  hooks: 'hooks',
  mcp: 'MCP servers',
}

// A plugin that runs programs itself - a program, or a Claude plugin's
// hooks and MCP servers - runs as the user, outside any sandbox. JS and
// WebAssembly plugins are sandboxed whatever they ask for.
export function notSandboxed(plugin) {
  return (
    (plugin.powers ?? []).includes('run_programs') &&
    (plugin.runtime === 'process' || plugin.runtime === 'content')
  )
}

// Where a plugin came from, in a line.
export function sourceLabel(source) {
  if (!source) return ''
  if (source.kind === 'folder') return `Folder: ${source.path}`
  if (source.kind === 'git') return `Git: ${source.url}`
  if (source.kind === 'archive') return `Archive: ${source.location}`
  if (source.kind === 'marketplace') return `${source.marketplace}: ${source.from}`
  return ''
}

// Plugins of one name that are on at once. Their skills and commands
// carry the same name, so a Claude chat may mix them up - the Installed
// tab and the home screen say so. Each clash: the name and its ids.
export function clashes(installed) {
  const byName = new Map()
  for (const p of installed ?? []) {
    if (!p.enabled) continue
    byName.set(p.name, [...(byName.get(p.name) ?? []), p.id])
  }
  return [...byName]
    .filter(([, ids]) => ids.length > 1)
    .map(([name, ids]) => ({ name, ids }))
}

// A catalog is fetched again when Discover opens and its copy is over a
// day old (or was never fetched). Nothing refreshes in the background.
export const STALE_MS = 24 * 60 * 60 * 1000

export function isStale(record, now = Date.now()) {
  return Boolean(record?.enabled) && (!record.refreshed_at || now - record.refreshed_at > STALE_MS)
}

// Discover's filters: every word somewhere in the name, description,
// author, category or tags; one category; one marketplace.
export function filterEntries(entries, filters = {}) {
  const { query = '', category = '', marketplace = '' } = filters ?? {}
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  return (entries ?? []).filter((e) => {
    if (!e) return false
    if (category && e.category !== category) return false
    if (marketplace && e.marketplace !== marketplace) return false
    if (!words.length) return true
    const tags = Array.isArray(e.tags) ? e.tags : []
    const hay = [e.name, e.display_name, e.description, e.author, e.category, ...tags]
      .filter(Boolean)
      .join(' ')
      .toLowerCase()
    return words.every((w) => hay.includes(w))
  })
}

export function categories(entries) {
  return [...new Set((entries ?? []).map((e) => e.category).filter(Boolean))].sort()
}

// Derive 1-2 uppercase characters from a plugin name for its tile monogram.
// Starts with two digits if present (e.g. '42crunch' -> '42'), initials of
// the first two words (e.g. 'agent-sdk' -> 'AS'), the first two letters of a
// single word ('docker' -> 'DO'), or 'PL' when empty or non-alphanumeric.
export function pluginInitials(name) {
  if (!name || typeof name !== 'string') return 'PL'
  const trimmed = name.trim()
  const leadingDigits = trimmed.match(/^(\d{2})/)
  if (leadingDigits) return leadingDigits[1]
  const words = trimmed.match(/[a-zA-Z0-9]+/g)
  if (!words || words.length === 0) return 'PL'
  if (words.length >= 2) {
    return (words[0][0] + words[1][0]).toUpperCase()
  }
  const single = words[0]
  if (single.length >= 2) {
    return single.slice(0, 2).toUpperCase()
  }
  return single.toUpperCase()
}

// Recognized tool and author brand keys mapped from plugin id or name.
// Maps github, gitlab, docker, android, aws, slack, pdf, jupyter, python, figma
// to canonical brand keys, or returns null.
export function resolvePluginBrand(id, name) {
  const text = `${id ?? ''} ${name ?? ''}`.toLowerCase()
  if (!text.trim()) return null

  if (text.includes('gitlab')) return 'gitlab'
  if (text.includes('github')) return 'github'
  if (text.includes('docker')) return 'docker'
  if (text.includes('android')) return 'android'
  if (text.includes('slack')) return 'slack'
  if (text.includes('jupyter')) return 'jupyter'
  if (text.includes('python')) return 'python'
  if (text.includes('figma')) return 'figma'
  if (/(?:^|[^a-z0-9])aws(?:[^a-z0-9]|$)/.test(text)) return 'aws'
  if (/(?:^|[^a-z0-9])pdf(?:[^a-z0-9]|$)/.test(text)) return 'pdf'

  return null
}

function titleCase(text) {
  if (!text) return ''
  return text
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase())
    .join(' ')
}

// Groups catalog into sorted category clusters. Missing or blank categories
// bucket into 'General' at the end. Categories with no entries are omitted.
export function groupEntriesByCategory(entries) {
  const map = new Map()
  for (const entry of entries ?? []) {
    if (!entry) continue
    const raw = (entry.category ?? '').trim()
    const cat = raw ? raw.toLowerCase() : 'general'
    if (!map.has(cat)) {
      map.set(cat, {
        category: cat,
        label: titleCase(cat),
        entries: [],
      })
    }
    map.get(cat).entries.push(entry)
  }
  return [...map.values()].sort((a, b) => {
    if (a.category === 'general' && b.category !== 'general') return 1
    if (b.category === 'general' && a.category !== 'general') return -1
    return a.label.localeCompare(b.label)
  })
}

// Search catalog entries by query, category, and marketplace. When a text
// query is provided, entries are scored by relevance (exact match > prefix
// match > description match > author/marketplace) with safe substring
// matching that handles regex characters without throwing. When query is
// absent, results sort alphabetically by display name.
export function searchEntries(entries, filters = {}) {
  const { query = '', category = '', marketplace = '' } = filters ?? {}
  const q = (query ?? '').trim().toLowerCase()
  const words = q.split(/\s+/).filter(Boolean)
  const catFilter = (category ?? '').trim().toLowerCase()

  const list = (entries ?? []).filter((e) => {
    if (!e) return false
    if (catFilter) {
      const eCat = (e.category ?? '').trim().toLowerCase()
      if (catFilter === 'general') {
        if (eCat && eCat !== 'general') return false
      } else if (eCat !== catFilter) {
        return false
      }
    }
    if (marketplace && e.marketplace !== marketplace) return false
    if (!words.length) return true

    const tags = Array.isArray(e.tags) ? e.tags : []
    const hay = [e.name, e.display_name, e.description, e.author, e.category, e.marketplace, ...tags]
      .filter(Boolean)
      .join(' ')
      .toLowerCase()
    return words.every((w) => hay.includes(w))
  })

  if (!q) {
    return list.sort((a, b) =>
      (a.display_name || a.name || '').localeCompare(b.display_name || b.name || ''),
    )
  }

  function score(e) {
    let s = 0
    const name = (e.name ?? '').toLowerCase()
    const disp = (e.display_name ?? '').toLowerCase()
    const desc = (e.description ?? '').toLowerCase()
    const author = (e.author ?? '').toLowerCase()
    const tags = (Array.isArray(e.tags) ? e.tags : []).map((t) => String(t).toLowerCase())

    // Exact matches
    if (name === q || disp === q) s += 1000
    // Prefix matches
    else if (name.startsWith(q) || disp.startsWith(q)) s += 500
    // Substring in name
    else if (name.includes(q) || disp.includes(q)) s += 200

    // Exact tag match
    if (tags.includes(q)) s += 150

    // Description contains full query
    if (desc.includes(q)) s += 50

    // Author match
    if (author.includes(q)) s += 30

    // Word-level scoring
    for (const w of words) {
      if (name.includes(w) || disp.includes(w)) s += 20
      if (tags.some((t) => t.includes(w))) s += 15
      if (desc.includes(w)) s += 10
    }

    return s
  }

  return list
    .map((e) => ({ entry: e, score: score(e) }))
    .sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score
      const nameA = a.entry.display_name || a.entry.name || ''
      const nameB = b.entry.display_name || b.entry.name || ''
      return nameA.localeCompare(nameB)
    })
    .map((item) => item.entry)
}

// A Discover entry's install, as the daemon takes it. A command entry
// stages only with the exact command the user was shown.
export function entrySource(entry, approveCommand = null) {
  return {
    kind: 'marketplace',
    marketplace: entry.marketplace,
    plugin: entry.name,
    ...(approveCommand ? { approve_command: approveCommand } : {}),
  }
}

// Where an entry stands: can't be installed, not installed, installed,
// or installed with something newer on offer.
export function entryState(entry) {
  if (!entry.installable) return 'unavailable'
  if (!entry.installed) return 'available'
  return entry.update ? 'update' : 'installed'
}

// When its catalog was last fetched, in a few words.
export function fetchedLabel(record, now = Date.now()) {
  if (!record?.refreshed_at) return 'Not fetched yet'
  const minutes = Math.round((now - record.refreshed_at) / 60_000)
  if (minutes < 1) return 'Fetched just now'
  if (minutes < 60) return `Fetched ${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `Fetched ${hours} h ago`
  return `Fetched ${Math.round(hours / 24)} d ago`
}

// The Install dialog's source as the daemon takes it; null until there
// is something to read.
export function sourceRequest(kind, value) {
  const v = (value ?? '').trim()
  if (!v) return null
  if (kind === 'folder') return { kind: 'folder', path: v }
  if (kind === 'git') return { kind: 'git', url: v }
  if (kind === 'archive') return { kind: 'archive', location: v }
  return null
}

// What the review says after an install attempt failed. The daemon takes
// the staged copy the moment an install starts, so a failed attempt
// leaves no stage id that could work again: the sentence sends the user
// back to reading the source, not to pressing Install twice.
export function installFailedNote(message) {
  const said = String(message ?? '').trim()
  const why = said || 'The install did not go through.'
  return `${why} Nothing was installed. The staged copy is used up as soon as an install starts, so read the same source again to retry.`
}

// ---- the Installed tab's search ----
//
// Installed is where a person goes when they have ten plugins and want one,
// and it is a flat list; Discover's search only searches the catalogs. The
// rule is the rules list's rule with one difference that matters here: a word
// has to appear inside ONE of the fields an installed row prints, not anywhere
// in a joined string, so a search can never match text assembled across two
// fields whose letters happen to sit side by side on the row - the thing you
// found is then visibly the thing you found.
//
// The fields are the row's own: the plugin's name, its runtime (the chip's
// words) and its source (the folder, the repo, or the marketplace and its
// package). Nothing hidden is searched: the install id, the powers, the path
// keys and the version are all printed or not, but none of them is a name a
// person would type to find a plugin.

/** The line the Installed tab shows when nothing is installed yet. */
export const INSTALLED_EMPTY_NONE =
  'Nothing installed yet. Find plugins under Discover, or install one from a folder, a git repo or an archive.'

// The count at which the field carries its hint. An installed row prints a
// description, an action row, a chips row and a source line, so it is about
// twice the height of a rule row in Automations (whose hint appears at six):
// four of these fit above the fold on a laptop, and five is one before the
// list stops fitting in a glance.
export const INSTALLED_SEARCH_HINT_FROM = 5

/** The words one installed row prints, one lowercased string per field. */
export function installedFields(plugin) {
  const runtime = RUNTIME_LABEL[plugin?.runtime] ?? plugin?.runtime ?? ''
  return [plugin?.name, runtime, sourceLabel(plugin?.source)]
    .filter((field) => typeof field === 'string' && field.trim().length > 0)
    .map((field) => field.toLowerCase())
}

/** Whether one installed plugin shows what the query asks for. */
export function matchesInstalled(plugin, query) {
  const words = String(query ?? '')
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
  if (!words.length) return true
  const fields = installedFields(plugin)
  return words.every((word) => fields.some((field) => field.includes(word)))
}

/** The plugins to draw, in the order the list already had them. */
export function searchInstalled(installed, query) {
  return (Array.isArray(installed) ? installed : []).filter((plugin) => matchesInstalled(plugin, query))
}

/** The quiet line beside the field while a query is on: "3 of 12 plugins". */
export function installedCountLine(shown, total) {
  return `${shown} of ${total} plugin${total === 1 ? '' : 's'}`
}

/** The answer when a query matches nothing: it names the query. */
export function installedEmptyLine(query) {
  const words = String(query ?? '').trim()
  return words ? `No plugins match “${words}”.` : INSTALLED_EMPTY_NONE
}

/**
 * The field's hint, or '' when the list is short enough that it needs none.
 * `total` is how many plugins are installed, not how many a search shows.
 */
export function installedSearchHint(total) {
  return (total ?? 0) >= INSTALLED_SEARCH_HINT_FROM ? 'Search installed plugins' : ''
}

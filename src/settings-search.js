// Settings sections and the index behind the settings search. Pure - no
// React, no DOM - so the ranking is tested directly under node:test.
//
// One entry per settings row: the label and detail read as the row reads,
// and `terms` carry the words a person might search for instead (theme,
// shortcut, port). A new row in SettingsView belongs here too, or the
// search cannot find it. Rows whose list is daemon data (harnesses, MCP
// servers, archived chats, model endpoints) are not written down here at
// all: `dynamicEntries` builds them from the same arrays the view draws, so
// a seventh harness or a new archived chat is searchable without an edit.

import { CATALOG, GROUPS, fuzzyScore } from './shortcuts.js'
import { harnessName } from './harness-names.js'
import { chatTitle, text } from './shape.js'

// The sidebar's category order. A section names its group by id; the tab
// list renders one small label per group.
export const SETTING_GROUPS = [
  { id: 'general', label: 'General' },
  { id: 'system', label: 'System' },
  { id: 'chats', label: 'Chats' },
  { id: 'about', label: 'About' },
]

export const SECTIONS = [
  { id: 'new-tasks', label: 'New tasks', icon: 'sparkle', group: 'general', phone: false },
  { id: 'appearance', label: 'Appearance', icon: 'palette', group: 'general', phone: true },
  { id: 'startup', label: 'Startup', icon: 'power', group: 'general', phone: false },
  { id: 'keyboard', label: 'Keyboard', icon: 'keyboard', group: 'general', phone: false },
  { id: 'cloud', label: 'Cloud', icon: 'cloud', group: 'system', phone: true },
  { id: 'harnesses', label: 'Apps', icon: 'robot', group: 'system', phone: false },
  { id: 'agents', label: 'Agents', icon: 'users', group: 'system', phone: false },
  { id: 'mcp-servers', label: 'MCP servers', icon: 'plugs', group: 'system', phone: false },
  { id: 'models', label: 'Models', icon: 'shapes', group: 'system', phone: false },
  { id: 'browser', label: 'Browser', icon: 'globe', group: 'system', phone: false },
  { id: 'archived', label: 'Archived chats', icon: 'archive', group: 'chats', phone: true },
  { id: 'this-install', label: 'This install', icon: 'info', group: 'about', phone: true },
]

// A section's own search result: the tab itself is a destination, even
// before any of its rows have data to show.
const BLURBS = {
  'new-tasks': 'What a new chat starts with.',
  appearance: 'How the console looks and moves.',
  startup: 'What the app does when it opens.',
  keyboard: 'Keys for every action.',
  cloud: 'Your account and this device.',
  harnesses: 'The apps OpenRemote can drive on this machine.',
  agents: "What a chat's agent may do with other chats: workers, how many, on which apps.",
  'mcp-servers': 'Tools a session can call.',
  models: 'Custom model endpoints on this computer, which a chat can run through.',
  browser: 'What the built-in browser keeps on this computer, and where to clear it.',
  archived: 'Chats hidden on this computer.',
  'this-install': 'The build you are running, its daemon, and the first-run steps.',
}

const SECTION_LABEL = Object.fromEntries(SECTIONS.map((s) => [s.id, s.label]))
const GROUP_LABEL = Object.fromEntries(GROUPS.map((g) => [g.id, g.label]))

const row = (key, section, label, detail, terms) => ({ key, section, label, detail, terms })

const ROWS = [
  row('new-tasks-harness', 'new-tasks', 'App', 'What a new chat starts on.', ['default', 'agent', 'cli', 'start', 'harness', 'app']),
  row('new-tasks-model', 'new-tasks', 'Model', "The app's own catalog.", ['default', 'catalog']),
  row('new-tasks-effort', 'new-tasks', 'Effort', "The model's own reasoning tiers.", ['reasoning', 'tier', 'thinking', 'depth']),
  row('new-tasks-permission', 'new-tasks', 'Permission', 'What the app may do without asking.', ['approval', 'ask', 'mode']),
  row('new-tasks-projects', 'new-tasks', 'Projects', 'The folders whose chats group under their project.', ['folder', 'workspace', 'group', 'remove', 'unregister', 'delete']),

  row('appearance-density', 'appearance', 'Compact density', 'Pulls the rows and gutters in.', ['spacing', 'layout', 'comfortable']),
  row('appearance-motion', 'appearance', 'Reduce motion', 'Stops the running-dot breathe and picker transitions.', ['animation', 'accessibility', 'transition']),
  row('appearance-privacy', 'appearance', 'Privacy mode', 'Blurs titles, paths, and the transcript for recordings.', ['blur', 'recording', 'stream', 'hide', 'screen']),

  row('startup-reopen', 'startup', 'Reopen', 'The view you left, or always New chat.', ['startup', 'last view', 'new chat', 'boot']),
  row('startup-launch', 'startup', 'Launch at login', 'Starts OpenRemote when you sign in to this computer.', ['autostart', 'boot', 'windows']),
  row('startup-tray', 'startup', 'Keep running in the tray', 'Closing the window leaves OpenRemote running.', ['background', 'close', 'quit', 'tray']),
  row('startup-above', 'startup', 'Keep above other windows', 'The window stays in front of other apps while OpenRemote is open.', ['always on top', 'topmost', 'above', 'front', 'z-order', 'window']),

  row('cloud-account', 'cloud', 'Account', "Your moreweb account and this device's place in it.", ['sign in', 'sign out', 'email', 'sync', 'device']),

  row('keyboard-stale', 'keyboard', 'Saved shortcut for an action this build no longer has', 'A key saved for an action that is gone, and the button that clears it.', ['shortcut', 'stale', 'unused', 'clear', 'saved']),
  // The bar's one button, and the only control in the section that changes
  // everything at once. It is on screen only while some key has been changed,
  // and the view hides this row with it (`absent`, SettingsView).
  row('keyboard-reset', 'keyboard', 'Reset every shortcut', 'Puts every key back to its default and touches nothing else.', ['reset', 'default', 'restore', 'shortcut', 'key', 'binding']),

  row('mcp-add', 'mcp-servers', 'Add an MCP server', 'Its launch command, as the machine would start it.', ['tool', 'add', 'command', 'server']),
  row('models-endpoint-add', 'models', 'Add a model endpoint', 'A base URL on this machine or network that lists models.', ['endpoint', 'gateway', 'custom', 'model', 'base url', 'relay', 'self-hosted']),

  // The daemon is part of this install, not a tab of its own: the rows read in
  // the section that says which build and which process are running, in the
  // order the section draws them.
  row('install-version', 'this-install', 'OpenRemote version', 'The build and the daemon it talks to.', ['about', 'build', 'release', 'update', 'daemon']),
  row('daemon-connection', 'this-install', 'Daemon connection', 'The local process the chats run through.', ['connect', 'connection', 'token', 'local']),
  row('daemon-connect', 'this-install', 'Connect by address', 'The address and token of a daemon you run yourself.', ['connect', 'address', 'token', 'manual', 'elsewhere']),
  row('daemon-chats', 'this-install', 'Chats on record', 'Sessions survive daemon restarts; stopped ones can resume.', ['sessions', 'record', 'history', 'resume']),
  row('daemon-address', 'this-install', 'Daemon address', 'The local address the console talks to.', ['url', 'port', 'data', 'folder', 'path']),
  row('daemon-restart', 'this-install', 'Restart', 'Stops running chats and starts a fresh daemon.', ['stop', 'start', 'daemon']),
  row('install-first-run', 'this-install', 'First-run steps', 'Shows them again. The chats stay.', ['onboarding', 'setup', 'replay', 'welcome']),

  // The built-in browser keeps four things on this computer. The facts come
  // first so a person can find out what is kept before they find the buttons
  // that take it back; each row says where the thing lives, and the two that
  // can only be cleared from the browser itself say so rather than offering a
  // button that would do nothing from here.
  row('browser-bookmarks', 'browser', 'Bookmarks', 'Pages kept on this computer, in this console. Nothing is sent anywhere.', ['bookmark', 'bookmarks', 'favourites', 'saved', 'star', 'kept', 'where', 'where kept']),
  row('browser-history', 'browser', 'Pages read', "This window's reading record, on this computer only. A private tab is never recorded.", ['history', 'visited', 'pages', 'reading', 'record', 'private', 'where']),
  row('browser-profile', 'browser', 'Cookies and site data', "The profile the browser's tabs share: cookies, site data, cache and service workers. Cleared per site from the browser's own menu.", ['cookies', 'cookie', 'site data', 'storage', 'cache', 'profile', 'sign out', 'clear']),
  row('browser-downloads', 'browser', 'Downloads', 'Files the tabs saved, in your Downloads folder. The list is the browser\'s own Downloads page.', ['downloads', 'download', 'files', 'saved', 'folder', 'where']),
  row('browser-history-clear', 'browser', 'Clear history', "Empties this window's reading record. It asks once.", ['history', 'clear', 'delete', 'empty', 'forget', 'visited', 'erase']),
  row('browser-bookmarks-clear', 'browser', 'Clear bookmarks', 'Removes every kept page. It asks once.', ['bookmarks', 'bookmark', 'clear', 'remove', 'delete', 'empty', 'favourites']),
  row('browser-downloads-open', 'browser', 'Show the last download', 'Opens the folder of the newest finished download.', ['downloads', 'download', 'open', 'show', 'reveal', 'folder', 'file']),
]

// Every action, the browser's too: Settings drops the Browser group's rows
// where there is no browser (its `absent` set).
const KEYBOARD = CATALOG.map((a) =>
  row(
    `keyboard-${a.id}`,
    'keyboard',
    a.label,
    `Shortcut in the ${GROUP_LABEL[a.group] ?? a.group} group.`,
    ['shortcut', 'hotkey', 'key', 'binding'],
  ),
)

// A section can be found by more than its own name: the words a person types
// for what lives inside it are the section's too, so "cookies" reaches the
// Browser tab before it reaches a row inside it.
const SECTION_TERMS = {
  browser: ['history', 'cookies', 'bookmarks', 'downloads', 'clear', 'profile'],
  // The daemon lives inside This install now: someone typing "daemon" should
  // not have to know that the tab it moved into is named after the build.
  'this-install': ['daemon', 'sidecar', 'build', 'update', 'first-run', 'onboarding'],
}

// The section's own result points at the tab's element (data-stg="<id>").
const SECTION_ROWS = SECTIONS.map((s) => row(s.id, s.id, s.label, BLURBS[s.id] ?? '', [s.id, ...(SECTION_TERMS[s.id] ?? [])]))

export const SETTINGS_ENTRIES = [...SECTION_ROWS, ...ROWS, ...KEYBOARD]

/**
 * The rows whose list is daemon data, built from the same arrays the view
 * draws. Nothing here names a harness, a chat or a server: a seventh harness
 * is searchable the moment it appears in `harnesses`, and an archived chat is
 * findable while it is archived and gone the moment it is restored.
 *
 * - one row per harness (its own name, and whether it is installed);
 * - one row per MCP server;
 * - one row per archived chat, plus the section's own Restore and Delete
 *   actions while there is anything to act on (a query for "delete" should
 *   reach the control that deletes, not a fuzzy word in a detail line);
 * - for every harness that can run custom endpoints: its endpoint block, one
 *   row per endpoint that harness actually holds (by the person's name for it
 *   and by its base URL's host), and the row that says where its API key is
 *   set. `endpoints` is the list EndpointManager hands up, either one array or
 *   a map of harness id to array.
 */
export function dynamicEntries({
  harnesses = null,
  mcpServers = null,
  archived = null,
  chats = {},
  endpoints = null,
} = {}) {
  const harnessList = Array.isArray(harnesses) ? harnesses : []
  const servers = Array.isArray(mcpServers) ? mcpServers : []
  const archivedRows = Array.isArray(archived) ? archived : []
  // The name the sidebar prints for the same chat, from the same reader: a
  // search row that said "A new task" where the list said something else would
  // be a second naming rule by the back door.
  const titleOf = (s) => chatTitle({ title: chats?.[s.id]?.title ?? s.title })

  const entries = [
    ...harnessList.map((h) => ({
      key: `harness-${h.id}`,
      section: 'harnesses',
      label: harnessName(h.id),
      detail: h.available ? 'Installed on this machine.' : 'This app is not installed on this machine.',
      terms: ['harness', 'app', 'apps', 'cli', 'install', 'agent', h.id],
    })),
    ...servers.map((p) => ({
      key: `mcp-${p.id}`,
      section: 'mcp-servers',
      label: p.name,
      detail: p.detail ?? 'An MCP server.',
      terms: ['mcp', 'server', 'tool'],
    })),
    ...archivedRows.map((s) => ({
      key: `archived-${s.id}`,
      section: 'archived',
      label: titleOf(s),
      detail: harnessName(s.harness),
      terms: ['archive', 'chat', 'restore', 'delete', 'hidden'],
    })),
  ]

  if (archivedRows.length > 0) {
    // The section's two actions, said in the words a person searches with.
    // They point at the section because each acts on a row inside it.
    entries.push(
      {
        key: 'archived-delete-action',
        section: 'archived',
        label: 'Delete an archived chat',
        detail: 'On the chat\'s own row under Archived chats. Synced copies go on each device\'s next sync.',
        terms: ['delete', 'remove', 'destroy', 'forget', 'archived', 'chat'],
      },
      {
        key: 'archived-restore-action',
        section: 'archived',
        label: 'Restore an archived chat',
        detail: 'On the chat\'s own row under Archived chats; it goes back to the list.',
        terms: ['restore', 'unarchive', 'unhide', 'archived', 'chat'],
      },
    )
  }

  for (const h of harnessList) {
    if (!h.custom_endpoint_supported) continue
    const name = harnessName(h.id)
    entries.push(
      {
        key: `models-${h.id}`,
        section: 'models',
        label: `Model endpoints for ${name}`,
        detail: 'Model endpoints this app can run through.',
        terms: ['endpoint', 'gateway', 'custom', 'model', 'relay', h.id],
      },
    )
    // One row per endpoint the table holds, by the name the person gave it and
    // by the host it points at: "my gateway" and "gateway.internal" are both
    // how they think of it, and neither is a harness or a section name.
    for (const entry of endpointRowsFor(h.id, endpoints)) {
      entries.push({
        key: `endpoint-${h.id}-${entry.id}`,
        section: 'models',
        label: text(entry.name) || 'Model endpoint',
        detail: [text(entry.base_url), entry.enabled === false ? 'Off' : null].filter(Boolean).join(' · '),
        terms: [
          'endpoint',
          'model endpoint',
          'model server',
          'gateway',
          name,
          h.id,
          hostOf(entry.base_url),
          text(entry.default_model),
        ].filter(Boolean),
      })
    }
    entries.push({
      key: `models-key-${h.id}`,
      section: 'models',
      // The key is typed on the endpoint's Edit row, which this lands near.
      label: `Endpoint API key (${name})`,
      detail: 'Set on the endpoint\'s Edit row. The daemon stores it; the console never shows it again.',
      terms: ['api key', 'key', 'token', 'secret', 'auth', 'endpoint', h.id],
    })
  }

  return entries
}

/** The host a base URL points at, for the search: "https://box:4000/v1" -> "box:4000". */
export function hostOf(baseUrl) {
  const url = text(baseUrl).trim()
  if (!url) return ''
  try {
    return new URL(url.includes('://') ? url : `http://${url}`).host
  } catch {
    return url.replace(/^[a-z]+:\/\//i, '').split('/')[0]
  }
}

/** One harness's endpoints, however the table's list reached the view. */
function endpointRowsFor(harnessId, endpoints) {
  const list = Array.isArray(endpoints) ? endpoints : endpoints?.[harnessId]
  return (Array.isArray(list) ? list : []).filter((entry) => entry && entry.id != null)
}

// Which searchable text matched, and how far down the row it sits: the
// label beats a keyword, a keyword beats the row's section name, and the
// detail line is the weakest.
const WEIGHT = { terms: -20, section: -30, detail: -40 }

function scoreEntry(query, entry) {
  const scores = []
  const label = fuzzyScore(query, entry.label)
  if (label !== null) scores.push(label)
  for (const term of entry.terms) {
    const score = fuzzyScore(query, term)
    if (score !== null) scores.push(score + WEIGHT.terms)
  }
  const section = fuzzyScore(query, SECTION_LABEL[entry.section] ?? '')
  if (section !== null) scores.push(section + WEIGHT.section)
  const detail = fuzzyScore(query, entry.detail)
  if (detail !== null) scores.push(detail + WEIGHT.detail)
  return scores.length ? Math.max(...scores) : null
}

export function searchSettings(query, entries = SETTINGS_ENTRIES) {
  const q = query.trim()
  if (!q) return []
  const words = q.split(/\s+/).filter(Boolean)
  return entries
    .map((entry, index) => {
      // The whole query first, so a phrase keeps its strong score. When that
      // finds nothing, every word must match somewhere on the same entry:
      // "install claude" is two words in two places (a term and the label),
      // and a person does not think in one string.
      let score = scoreEntry(q, entry)
      if (score === null && words.length > 1) {
        const each = words.map((word) => scoreEntry(word, entry))
        score = each.every((part) => part !== null)
          ? each.reduce((sum, part) => sum + part, 0) / each.length
          : null
      }
      return { entry, index, score }
    })
    .filter((r) => r.score !== null)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((r) => r.entry)
}

export function visibleSections(mobile) {
  return mobile ? SECTIONS.filter((s) => s.phone) : SECTIONS
}

// ---- the keyboard's share of the search ----

/**
 * The row Enter in the search field opens, or null when there is nothing to
 * open: Enter on an empty list must do nothing rather than act on the last
 * answer that was on screen.
 *
 * `highlighted` is the key of a row a roving highlight has landed on. The list
 * keeps no highlight of its own today (Down moves focus into the rows, and a
 * focused row is a button that answers its own Enter), so it is null and the
 * first result answers. A key that is not in `shown` is stale - the query
 * changed under it - and falls back to the first row rather than opening a row
 * the person can no longer see.
 */
export function resultToOpen(shown, highlighted = null) {
  const rows = Array.isArray(shown) ? shown : []
  if (!rows.length) return null
  if (highlighted != null) {
    const hit = rows.find((entry) => entry?.key === highlighted)
    if (hit) return hit
  }
  return rows[0] ?? null
}

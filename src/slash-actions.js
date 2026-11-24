// The App's own / commands as the console runs them. The daemon says how
// each command runs (GET /commands: `route` and `action`); this file holds
// the console's side of that: which rows the console runs itself, the
// refusal for a command that only works in the App's terminal, and what
// /help and /status show. Pure: tested without a DOM.

// The actions a `console` row can name. The daemon's table names only these
// (slash_tables.rs, CONSOLE_ACTIONS); the test keeps the two lists equal.
export const CONSOLE_ACTIONS = [
  'new',
  'stop',
  'resume',
  'help',
  'status',
  'copy',
  'export',
  'changes',
  'plugins',
  'mcp',
  'apps',
  'settings',
  'model',
  'effort',
  'fast',
  'permissions',
  'rename',
]

// Where the view-opening actions go: a view of the app, or a section of
// Settings.
export const ACTION_TARGETS = {
  plugins: { view: 'plugins' },
  mcp: { settings: 'mcp-servers' },
  apps: { settings: 'harnesses' },
  settings: { settings: null },
}

// The command a message starts with: its name (no slash) and the rest. A
// path (`/usr/bin`) is not a command.
export function leadingCommand(text) {
  const match = /^\s*\/(\S+)(?:\s+([\s\S]*))?$/.exec(text ?? '')
  if (!match || match[1].includes('/')) return null
  return { name: match[1], args: (match[2] ?? '').trim() }
}

function byName(commands, name) {
  return (commands ?? []).find((c) => c.name === name || (c.aliases ?? []).includes(name)) ?? null
}

// The App's rows the console runs itself, shaped like OpenRemote's own
// commands so the composer's send path takes them out of the message.
// `arg` is the choice list the argument comes from, when it has one.
export function consoleCommands(commands = []) {
  const out = []
  for (const c of commands) {
    if (c.route !== 'console' || !CONSOLE_ACTIONS.includes(c.action)) continue
    for (const name of [c.name, ...(c.aliases ?? [])]) {
      out.push({
        name,
        action: c.action,
        arg: c.choices ?? (c.action === 'rename' ? 'title' : undefined),
        description: c.description ?? '',
        app: true,
      })
    }
  }
  return out
}

// The sentence for a message whose command cannot run here, or null. The
// daemon refuses the same message; saying it here keeps the draft in the
// composer and costs no round trip.
export function refusalFor(text, commands = []) {
  const lead = leadingCommand(text)
  if (!lead) return null
  const row = byName(commands, lead.name)
  if (!row || row.route !== 'refuse') return null
  return `/${row.name} ${row.why ?? 'only works in the App\'s own terminal window'}`
}

const SOURCE_WORDS = {
  builtin: 'Built in',
  custom: 'Your commands',
  harness: 'From the App',
  plugin: 'Plugins',
  mcp: 'MCP prompts',
}

// /help: every command the menu knows, grouped the way the menu badges
// them, each with how it runs here. OpenRemote's own come first.
export function helpGroups({ own = [], commands = [] } = {}) {
  const groups = []
  if (own.length) {
    groups.push({
      title: 'OpenRemote',
      rows: own.map((c) => ({ name: `/${c.name}`, detail: c.description ?? '', how: 'Runs in OpenRemote' })),
    })
  }
  const ownNames = new Set(own.map((c) => c.name))
  const buckets = new Map()
  for (const c of commands) {
    if (ownNames.has(c.name)) continue
    const title = c.kind === 'skill' ? 'Skills' : (SOURCE_WORDS[c.source] ?? 'From the App')
    if (!buckets.has(title)) buckets.set(title, [])
    buckets.get(title).push({
      name: `/${c.name}`,
      detail: c.description ?? '',
      hint: c.argument_hint ?? '',
      how: howItRuns(c),
      disabled: c.available === false,
    })
  }
  const order = ['Built in', 'Your commands', 'Skills', 'From the App', 'Plugins', 'MCP prompts']
  for (const title of order) {
    const rows = buckets.get(title)
    if (rows?.length) groups.push({ title, rows: rows.sort((a, b) => a.name.localeCompare(b.name)) })
  }
  return groups
}

export function howItRuns(c) {
  if (c.available === false) return c.why ?? 'Not available here'
  if (c.route === 'console') return 'Runs in OpenRemote'
  if (c.route === 'refuse') return c.why ?? 'Only in the App\'s terminal'
  return 'Sent to the App'
}

// Narrow /help's groups to a search, by name or description.
export function filterHelp(groups, query) {
  const q = (query ?? '').trim().toLowerCase()
  if (!q) return groups
  return groups
    .map((g) => ({
      ...g,
      rows: g.rows.filter((r) => r.name.toLowerCase().includes(q) || r.detail.toLowerCase().includes(q)),
    }))
    .filter((g) => g.rows.length)
}

// /status: what OpenRemote knows about this chat, as label and value rows.
export function statusRows(chat, { app = null, permissionLabel = null, modelLabel = null, device = null } = {}) {
  const rows = [['App', app ?? chat?.harness ?? 'Unknown']]
  if (chat?.model) rows.push(['Model', modelLabel ?? chat.model])
  if (chat?.effort) rows.push(['Effort', chat.effort])
  if (chat?.fast != null) rows.push(['Fast mode', chat.fast ? 'On' : 'Off'])
  const mode = chat?.permissionMode ?? chat?.permission_mode
  if (mode) rows.push(['Permissions', permissionLabel ?? mode])
  rows.push(['Folder', chat?.workspace ?? 'Home folder (no project)'])
  if (device) rows.push(['Runs on', device])
  if (chat?.status) rows.push(['Status', chat.status])
  const used = chat?.context?.used
  if (used) {
    const window = chat.context.window
    rows.push(['Context', window ? `${used.toLocaleString('en-US')} of ${window.toLocaleString('en-US')} tokens` : `${used.toLocaleString('en-US')} tokens`])
  }
  if (chat?.costUsd > 0) rows.push(['Cost so far', `$${chat.costUsd.toFixed(4)}`])
  if (chat?.thinkingTokens > 0) rows.push(['Thinking tokens', chat.thinkingTokens.toLocaleString('en-US')])
  if (chat?.approvedTools?.length) rows.push(['Allowed for this chat', chat.approvedTools.join(', ')])
  return rows
}

// The recent / commands, kept per App: what was used on one App says
// nothing about another.
export function recentKey(harness) {
  return harness ? `openremote-recent-commands:${harness}` : 'openremote-recent-commands'
}

// The last answer the App gave in a chat, for /copy: its newest assistant
// message, or the one still arriving.
export function lastAnswer(chat) {
  const items = Array.isArray(chat?.timeline) ? chat.timeline : []
  for (let i = items.length - 1; i >= 0; i -= 1) {
    const item = items[i]
    if (item?.kind === 'stream' && item.text) return String(item.text)
    if (item?.kind === 'message' && item.role !== 'user' && item.role !== 'note' && item.text) return String(item.text)
  }
  return null
}

// The argument hint for a command just picked, while nothing has been
// typed after it: `/review ` shows `[instructions]`.
export function pendingHint(text, caret, commands = []) {
  if (caret !== text.length || !/^\s*\/\S+ $/.test(text)) return null
  const lead = leadingCommand(text)
  const row = lead && commands.find((c) => c.name === lead.name || (c.aliases ?? []).includes(lead.name))
  return row?.argument_hint ? { name: row.name, hint: row.argument_hint } : null
}

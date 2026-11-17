// The composer's / and @ menus: when one opens, what it offers for what
// was typed, what a pick puts in the text, and - when the message is sent
// - the OpenRemote commands taken out of it to run in the app. A pick is
// plain text: what you see is what is sent. Pure: tested without a DOM.
import { maskReferences, normalizeAuthoredText } from './document-reference.js'

// OpenRemote's own commands. They run in the app when the message is
// sent and never reach the harness; on a name the harness also has
// (claude's /model, /effort, /fast, /rename) OpenRemote's takes over, so
// the app's own model, effort and name stay true. `arg` names what
// follows the command; `chat` ones need a running chat; `cloud` ones need
// Cloud.
export const OPENREMOTE_COMMANDS = [
  { name: 'model', arg: 'model', description: 'Switch the model for this chat' },
  { name: 'effort', arg: 'effort', description: 'Set how hard the model thinks' },
  { name: 'fast', arg: 'fast', description: 'Turn fast mode on or off' },
  { name: 'permissions', arg: 'permission', description: 'Set the permission mode' },
  { name: 'stop', description: 'Stop the chat', chat: true },
  { name: 'resume', description: 'Resume a stopped chat', chat: true },
  { name: 'rename', arg: 'title', description: 'Rename the chat: /rename "New name"' },
  { name: 'sync', description: 'Put the chat on Cloud - your other devices see it after their next sync, and it keeps running here', cloud: true },
  { name: 'cloud', description: 'Put the chat on Cloud (the same as /sync)', cloud: true },
  // Every App gets these, whatever its own list says: each is something
  // OpenRemote shows or does better than a terminal could.
  { name: 'new', action: 'new', description: 'Start a new chat with this App and folder', chat: true },
  { name: 'help', action: 'help', description: 'List every command this App has here' },
  { name: 'status', action: 'status', description: "Show this chat's App, model and settings", chat: true },
  { name: 'copy', action: 'copy', description: 'Copy the last answer', chat: true },
  { name: 'export', action: 'export', description: 'Copy the whole chat as Markdown', chat: true },
  { name: 'diff', action: 'changes', description: 'Show the files this chat changed', chat: true },
]

const OWN = new Map(OPENREMOTE_COMMANDS.map((c) => [c.name, c]))

// The words of a text with where each starts and ends; a "quoted string"
// is one word.
export function words(text) {
  const out = []
  const re = /"[^"]*"|\S+/g
  let match
  while ((match = re.exec(text))) out.push({ word: match[0], start: match.index, end: match.index + match[0].length })
  return out
}

// The menu the caret is in, if any: a / or @ word at the caret that
// starts the message or follows whitespace (both open anywhere), or the
// argument of an OpenRemote command just picked (`/model ` then a model).
export function menuAt(text, caret, extra = []) {
  let start = caret
  while (start > 0 && !/\s/.test(text[start - 1])) start -= 1
  const word = text.slice(start, caret)
  if (word.startsWith('/') && !word.slice(1).includes('/')) {
    return { kind: '/', query: word.slice(1), start, end: caret }
  }
  if (word.startsWith('@')) {
    return { kind: '@', query: word.slice(1).replace(/^"/, ''), start, end: caret }
  }
  // The word before: an OpenRemote command that takes a pickable argument.
  const before = text.slice(0, start).trimEnd()
  const previous = before.slice(before.search(/\S+$/))
  const name = previous.startsWith('/') ? previous.slice(1) : null
  const command = name ? (OWN.get(name) ?? extra.find((c) => c.name === name) ?? null) : null
  if (command?.arg && command.arg !== 'title' && before.length < start) {
    return { kind: 'arg', command: command.name, arg: command.arg, query: word, start, end: caret }
  }
  return null
}

// The key of the menu the caret is in: the kind and where its word starts.
// Two menus of the same kind at the same offset are the same word.
export function menuKeyOf(menu) {
  return menu ? `${menu.kind}:${menu.start}` : null
}

// The dismissal an Esc leaves behind, carried into the next render: it only
// covers the word it was pressed in. Once the caret has left that word the
// dismissal is spent, so Esc closes a menu for now and never for the rest of
// the composer - the next "/" or "@" opens again.
export function holdDismissal(menu, dismissed) {
  return menuKeyOf(menu) === dismissed ? dismissed : null
}

// Put a pick in place of the menu's word. A trailing space follows,
// unless the insertion wants the caret right after it (an MCP server's
// `@server:` waits for its resource).
export function applyPick(text, menu, insertion, { space = true } = {}) {
  const after = text.slice(menu.end)
  const glue = space && !after.startsWith(' ') ? ' ' : ''
  const next = text.slice(0, menu.start) + insertion + glue + after
  return { text: next, caret: menu.start + insertion.length + glue.length }
}

// What a pick inserts.
export function fileInsertion(path) {
  return /\s/.test(path) ? `@"${path}"` : `@${path}`
}

// Claude's own agent mention (`@agent-<name>`, a plugin's scoped
// `@agent-plugin:name`); other harnesses get the name as written.
export function agentInsertion(name, harness) {
  return harness === 'claude' ? `@agent-${name}` : `@${name}`
}

// A chat pointer: its name and the start of its id, which the agent hands
// to the read-a-chat tool. Quotes in the name are dropped so the pointer
// stays one readable phrase.
export function chatInsertion(chat) {
  const title = (chat.title ?? 'untitled').replace(/"/g, '').slice(0, 60)
  return `@chat "${title}" (${String(chat.id).slice(0, 8)})`
}

const CHAT_POINTER = /@chat "[^"]*" \(([0-9a-fA-F-]{6,})\)/g

// The console keeps its chats keyed by id; a plain list reads the same.
export function chatList(chats) {
  if (!chats) return []
  return Array.isArray(chats) ? chats : Object.values(chats)
}

// The chats a message points at: each pointer's id start, matched to a
// chat this console knows.
export function mentionedChats(text, chats) {
  const ids = new Set()
  for (const match of maskReferences(text).matchAll(CHAT_POINTER)) {
    const found = chatList(chats).find((c) => String(c.id).startsWith(match[1]))
    if (found) ids.add(found.id)
  }
  return [...ids]
}

// Which OpenRemote commands this composer offers: no chat-only ones before
// a chat exists, no Cloud ones without Cloud.
export function ownCommands({ inChat = true, cloud = false } = {}) {
  return OPENREMOTE_COMMANDS.filter((c) => (inChat || !c.chat) && (cloud || !c.cloud))
}

// The query as the starts of the name's words, each start optionally
// followed by more of that word: `/sr` and `/secrev` find
// `/security-review`, while letters scattered inside words (`deep` in
// `auto-mode-setup`) do not count.
export function fuzzyScore(query, name) {
  const q = query.toLowerCase()
  const n = name.toLowerCase()
  const starts = (i) => i === 0 || /[-_:\s]/.test(n[i - 1])
  let at = 0
  let first = true
  for (const ch of q) {
    if (!first && n[at] === ch) {
      at += 1
      continue
    }
    let found = -1
    for (let i = at; i < n.length; i += 1) {
      if (n[i] === ch && starts(i)) {
        found = i
        break
      }
    }
    if (found < 0) return 0
    at = found + 1
    first = false
  }
  return 1.5
}

function rankBy(query, name, description = '') {
  const q = query.toLowerCase()
  const n = name.toLowerCase()
  if (!q) return 1
  if (n.startsWith(q)) return 3
  if (n.includes(q)) return 2
  const fuzzy = q.length > 1 ? fuzzyScore(q, n) : 0
  if (fuzzy) return fuzzy
  if (description.toLowerCase().includes(q)) return 1
  return 0
}

// A harness reports everything it knows, the user's whole skill library
// and its own internals included; a menu of a hundred rows helps nobody.
// What reads as a skill (a plugin-scoped name, or a description written
// as "use this skill when...") is kept apart from the commands, names the
// harness keeps for itself (a leading underscore) and ones it calls
// removed or deprecated are left out, and descriptions are cut to one
// short line.
const SKILL_WORDS = /^(use (this skill|when)|read this skill|this skill)\b/i
const NOISE_WORDS = /^\((removed|deprecated)\)|\bdeprecated\b/i

// The harness commands most people reach for, shown before any typing
// when the harness has them.
const EVERYDAY = ['compact', 'init', 'review', 'code-review', 'security-review', 'context', 'usage', 'goal']

// The badge a row carries, so the user can tell the App's /review from
// their own /review-pr.
const BADGES = { plugin: 'plugin', mcp: 'MCP', custom: 'custom' }

function shortDescription(text = '') {
  const first = text.split(/(?<=[.!?])\s/)[0].trim()
  return first.length > 80 ? `${first.slice(0, 79).trimEnd()}…` : first
}

// An argument hint worth showing: short and plain, not a usage line.
function shortHint(hint = '') {
  return hint && hint.length <= 24 && !hint.includes('--') ? hint : ''
}

export function commandKind(c) {
  if (c.name.startsWith('_') || NOISE_WORDS.test(c.description ?? '')) return 'hidden'
  // The machine knows which are skills (their folders); older daemons
  // don't say, and the wording tells.
  if (c.kind === 'skill' || c.kind === 'command') return c.kind
  if (c.name.includes(':') || SKILL_WORDS.test(c.description ?? '')) return 'skill'
  return 'command'
}

function commandRow(c, group, score) {
  return {
    key: `cmd:${c.name}`,
    kind: 'command',
    group,
    label: `/${c.name}`,
    detail: shortDescription(c.description),
    hint: shortHint(c.argument_hint),
    badge: BADGES[c.source] ?? null,
    disabled: c.available === false,
    why: c.why ?? null,
    route: c.route ?? 'native',
    insertion: `/${c.name}`,
    score,
  }
}

function note(text) {
  return { key: `note:${text}`, kind: 'note', note: true, detail: text, disabled: true, insertion: '' }
}

// The / menu. Before any typing: OpenRemote's own commands, the ones used
// lately, a few everyday harness commands, and how much more a search
// finds. While typing: the best matches, grouped, a dozen or so at most.
// `unavailable` is the line the menu shows in place of a list that could not
// be read: an empty menu that says so beats another chat's rows.
export function commandItems(query, { harnessCommands = [], own = [], recent = [], unavailable = null } = {}) {
  const taken = new Set(own.map((c) => c.name))
  const known = harnessCommands.filter((c) => !taken.has(c.name) && commandKind(c) !== 'hidden')
  const commands = known.filter((c) => commandKind(c) === 'command')
  const skills = known.filter((c) => commandKind(c) === 'skill')
  const ownRow = (c, score) => ({
    key: `own:${c.name}`,
    kind: 'openremote',
    group: 'OpenRemote',
    label: `/${c.name}`,
    detail: c.description,
    hint: '',
    badge: null,
    disabled: false,
    why: null,
    insertion: `/${c.name}`,
    score,
  })

  if (!query) {
    const items = own.map((c) => ownRow(c, 1))
    const byName = new Map(known.map((c) => [c.name, c]))
    const lately = recent.filter((n) => byName.has(n)).slice(0, 5)
    for (const name of lately) items.push(commandRow(byName.get(name), 'Recent', 1))
    for (const name of EVERYDAY) {
      if (lately.includes(name) || !byName.has(name)) continue
      if (commandKind(byName.get(name)) === 'command') items.push(commandRow(byName.get(name), 'Commands', 1))
    }
    const shown = items.length - own.length
    const more = commands.length + skills.length - shown
    if (more > 0) {
      const parts = []
      if (commands.length) parts.push(`${commands.length} commands`)
      if (skills.length) parts.push(`${skills.length} skills`)
      items.push(note(`Type to search ${parts.join(' and ')}`))
    }
    if (unavailable) items.push(note(unavailable))
    return items
  }

  const scored = (list) =>
    list
      .map((c) => ({
        c,
        score: Math.max(...[c.name, ...(c.aliases ?? [])].map((n) => rankBy(query, n, c.description ?? ''))),
      }))
      .filter((x) => x.score)
      .sort((a, b) => b.score - a.score || a.c.name.localeCompare(b.c.name))
  const ownHits = own
    .map((c) => ({ c, score: rankBy(query, c.name, c.description) }))
    .filter((x) => x.score)
    .sort((a, b) => b.score - a.score || a.c.name.localeCompare(b.c.name))
  const commandHits = scored(commands)
  const skillHits = scored(skills)
  const items = [
    ...ownHits.map((x) => ownRow(x.c, x.score)),
    ...commandHits.slice(0, 6).map((x) => commandRow(x.c, 'Commands', x.score)),
    ...skillHits.slice(0, 6).map((x) => commandRow(x.c, 'Skills', x.score)),
  ]
  const hidden = Math.max(0, commandHits.length - 6) + Math.max(0, skillHits.length - 6)
  if (hidden > 0) items.push(note(`${hidden} more - keep typing to narrow`))
  if (unavailable) items.push(note(unavailable))
  return items
}

// Whether `query` is in `path` the way the daemon looks for it: every
// character, in order, anywhere (files.rs ranks by subsequence, so a row
// that passes here is one the daemon could have returned).
function inOrder(path, query) {
  const text = path.toLowerCase()
  let at = 0
  for (const ch of query.toLowerCase()) {
    at = text.indexOf(ch, at) + 1
    if (at === 0) return false
  }
  return true
}

// The files the menu may show for the word being typed. The answer for this
// exact word is the list. A word that is still being typed keeps the rows of
// the answer before it, narrowed to what still matches: clearing them on
// every keystroke emptied the menu and it came back a beat later, which
// reads as a flicker. A different word has nothing, and a word whose ask
// failed has none: the previous word's files under it would read as a match.
export function filesForQuery(held, query) {
  if (!held) return []
  if (held.q === query) return held.entries
  if (!String(query).startsWith(held.q)) return []
  return held.entries.filter((f) => inOrder(f.path, query))
}

// The @ menu: files and folders, agents, MCP servers, other chats -
// each group filtered by the query.
//
// Files are narrowed here as well as by the ask: the ask's answer is a beat
// behind the typing, and a daemon that answers with a wider list than was
// asked for would otherwise put every file it knows under a word that matches
// almost none of them. `inOrder` is the matcher the file ask already ranks by
// (the daemon looks for the characters of the word, in order, anywhere in the
// path), so a row that survives here is one the ask could have returned.
//
// `unavailable` is the line for a list the machine did not answer;
// `filesUnavailable` is the same for the files, which are asked separately and
// fail separately. Both are read, never picked.
const MENTION_ROWS = 80

export function mentionItems(
  query,
  {
    files = [],
    agents = [],
    servers = [],
    chats = [],
    harness = '',
    unavailable = null,
    filesUnavailable = null,
  } = {},
) {
  const items = []
  for (const f of files) {
    if (!inOrder(f.path, query)) continue
    items.push({
      key: `file:${f.path}`,
      kind: 'file',
      group: 'Files',
      label: f.kind === 'dir' ? `${f.path}/` : f.path,
      detail: '',
      insertion: fileInsertion(f.path),
      disabled: false,
    })
  }
  for (const a of agents) {
    if (!rankBy(query, a.name, a.description ?? '')) continue
    items.push({
      key: `agent:${a.name}`,
      kind: 'agent',
      group: 'Agents',
      label: a.name,
      detail: a.description ?? '',
      badge: a.source === 'plugin' ? 'plugin' : null,
      insertion: agentInsertion(a.name, harness),
      disabled: a.available === false,
      why: a.why ?? null,
    })
  }
  for (const s of servers) {
    if (!rankBy(query, s.name)) continue
    items.push({
      key: `mcp:${s.name}`,
      kind: 'mcp',
      group: 'MCP servers',
      label: `${s.name}:`,
      detail: s.source === 'plugin' ? `from ${s.plugin}` : 'set up in Settings',
      insertion: `@${s.name}:`,
      space: false,
      disabled: false,
    })
  }
  for (const c of chats) {
    if (!rankBy(query, c.title ?? '')) continue
    items.push({
      key: `chat:${c.id}`,
      kind: 'chat',
      group: 'Chats',
      label: c.title ?? 'untitled',
      detail: c.harness ?? '',
      insertion: chatInsertion(c),
      disabled: false,
    })
  }
  // The budget is for rows, and a line that says the list is short has to fit
  // inside it: eighty filled rows pushing the line off the end makes a partial
  // list read as the whole answer. The failure lines come last and are counted
  // first, so a menu with two of them still ends with both.
  const notes = []
  if (filesUnavailable) notes.push(note(filesUnavailable))
  if (unavailable) notes.push(note(unavailable))
  const room = MENTION_ROWS - notes.length
  let rows = items.slice(0, room)
  let hidden = items.length - rows.length
  if (hidden > 0) {
    // One of the slots is the sentence, not a row.
    rows = items.slice(0, room - 1)
    hidden = items.length - rows.length
    rows = [...rows, note(`${hidden} more - keep typing to narrow`)]
  }
  return [...rows, ...notes]
}

// The argument menu after an OpenRemote command: the choices it takes.
export function argItems(query, choices = []) {
  return choices
    .filter((c) => rankBy(query, c.id, c.name ?? ''))
    .map((c) => ({
      key: `arg:${c.id}`,
      kind: 'arg',
      label: c.name ?? c.id,
      detail: c.detail ?? '',
      insertion: c.id,
      disabled: false,
    }))
}

// At send: the OpenRemote commands in the message, in order, each with its
// argument, and the message without them. Anything else - the harness's
// own commands included - stays as typed.
export function takeCommands(text, own = OPENREMOTE_COMMANDS) {
  const names = new Map(own.map((c) => [c.name, c]))
  const list = words(maskReferences(text))
  const run = []
  const cut = []
  for (let i = 0; i < list.length; i += 1) {
    const { word, start, end } = list[i]
    if (!word.startsWith('/')) continue
    const command = names.get(word.slice(1))
    if (!command) continue
    let stop = end
    let arg = null
    // /fast's on/off is optional: the next word is its argument only when
    // it is one of the two.
    const takesNext =
      command.arg &&
      list[i + 1] &&
      (command.arg !== 'fast' || /^(on|off)$/i.test(list[i + 1].word)) &&
      // An App's console row with no choice list (/rename) takes the rest.
      !list[i + 1].word.startsWith('/')
    if (takesNext) {
      arg = list[i + 1].word.replace(/^"|"$/g, '')
      stop = list[i + 1].end
      i += 1
    }
    run.push(command.action && command.action !== command.name ? { name: command.name, action: command.action, arg } : { name: command.name, arg })
    cut.push([start, stop])
  }
  let rest = text
  for (const [start, stop] of cut.reverse()) rest = rest.slice(0, start) + rest.slice(stop)
  return { run, text: normalizeAuthoredText(rest) }
}

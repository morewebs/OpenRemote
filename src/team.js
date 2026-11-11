// Agent teams, as the console reads them. A team is a tree of ordinary
// chats: a worker is a chat whose session carries a `parent` (the chat that
// started it, its depth below the user's own chat, the first words of its
// brief). Everything here is derived from the session list the console
// already polls - nothing about a team is stored on this side. Pure, so it
// is tested without a DOM.

import { harnessName } from './harness-names.js'

const TOOL = 'mcp__openremote__agents_'

/** A chat's alive states: a process is attached, even between turns. */
export const ALIVE = ['starting', 'working', 'waiting', 'idle']

export function isTeamTool(name) {
  return typeof name === 'string' && name.startsWith(TOOL)
}

/** Results that fold by default: a worker's whole transcript runs long. */
export function foldsTeamResult(name) {
  return name === `${TOOL}read` || name === `${TOOL}wait`
}

/** The sessions started directly by a chat, in the order they were started. */
export function workersOf(sessions, chatId) {
  return (sessions ?? [])
    .filter((s) => s.parent?.chat === chatId)
    .sort((a, b) => (a.parent.spawned_at ?? 0) - (b.parent.spawned_at ?? 0))
}

/**
 * Everything under a chat, parents before their own workers, each with its
 * depth below the chat (a direct worker is 1). A loop in the links - which
 * the daemon never makes - ends where it closes.
 */
export function descendantsOf(sessions, chatId) {
  const out = []
  const seen = new Set([chatId])
  const walk = (id, depth) => {
    for (const worker of workersOf(sessions, id)) {
      if (seen.has(worker.id)) continue
      seen.add(worker.id)
      out.push({ session: worker, depth })
      walk(worker.id, depth + 1)
    }
  }
  walk(chatId, 1)
  return out
}

/** The chat that started this one, when it is on this list. */
export function leadOf(sessions, chat) {
  const id = chat?.parent?.chat
  return id ? ((sessions ?? []).find((s) => s.id === id) ?? null) : null
}

/** Whether a session has a process attached. */
export function isAlive(session) {
  return ALIVE.includes(session?.status)
}

// The dot language: waiting beats failed beats running.
const RANK = { waiting: 3, failed: 2, running: 1, starting: 1 }
// A session's status in the dot vocabulary the sidebar and the chat use.
const DOT = { working: 'running' }
const dotOf = (status) => DOT[status] ?? status

/**
 * What a lead's dot shows: the heaviest of its own status and its
 * workers' (all in the dot vocabulary - running, waiting, failed, idle,
 * stopped, starting): amber when anyone waits for the user, red when
 * something failed, green while something runs.
 */
export function rolledStatus(own, kids = []) {
  let best = own
  for (const status of kids) if ((RANK[status] ?? 0) > (RANK[best] ?? 0)) best = status
  return best
}

/**
 * The dot a chat wears for what is under it alone - none when nothing
 * under it needs a mark. Meant for the team bar's head.
 */
export function teamDot(sessions, chatId) {
  const best = rolledStatus(
    null,
    descendantsOf(sessions, chatId).map(({ session }) => dotOf(session.status)),
  )
  return RANK[best] ? best : null
}

/** What a worker row says of its state, in the words an agent reads too. */
export function stateWord(status) {
  switch (status) {
    case 'starting':
    case 'working':
      return 'working'
    case 'waiting':
      return 'waiting for you'
    case 'idle':
      return 'idle'
    case 'failed':
      return 'failed'
    case 'stopped':
      return 'stopped'
    default:
      return status ?? ''
  }
}

/** How many of a chat's team are running a turn (working or waiting). */
export function runningCount(sessions, chatId) {
  return descendantsOf(sessions, chatId).filter(({ session }) =>
    ['starting', 'working', 'waiting'].includes(session.status),
  ).length
}

/** The one-line summary of a team: "2 working, 1 waiting for you". */
export function teamSummary(sessions, chatId) {
  const counts = {}
  for (const { session } of descendantsOf(sessions, chatId)) {
    const word = stateWord(session.status)
    counts[word] = (counts[word] ?? 0) + 1
  }
  const order = ['working', 'waiting for you', 'idle', 'failed', 'stopped']
  return order
    .filter((word) => counts[word])
    .map((word) => `${counts[word]} ${word}`)
    .join(', ')
}

/** A session's name as the team shows it. */
export function nameOf(session) {
  return session?.title?.trim() || 'A new task'
}

/** An agent named by its id or the start of it, found on the list. */
export function findAgent(sessions, wanted) {
  const text = String(wanted ?? '').trim().replace(/^chat:/, '')
  if (!text) return null
  return (
    (sessions ?? []).find((s) => s.id === text) ??
    (text.length >= 6 ? (sessions ?? []).find((s) => s.id.startsWith(text)) : null) ??
    null
  )
}

const firstLine = (text) => String(text ?? '').trim().split('\n')[0].trim()

const cut = (text, n) => (text.length > n ? `${text.slice(0, n).trimEnd()}…` : text)

/**
 * How a team tool call reads in a tool card or an approval: `{ label,
 * detail, mono }`. `ask` is the approval's tense ("Start a Codex worker")
 * against the card's ("Started a Codex worker"). `sessions` and `chatId`
 * (the chat that made the call) let it name the agents it mentions.
 * `result` is the call's output once it has landed.
 */
export function teamTool(name, input, { ask = false, sessions = [], chatId = null, result = null } = {}) {
  const verb = String(name).slice(TOOL.length)
  const args = input && typeof input === 'object' ? input : {}
  const target = () => findAgent(sessions, args.agent)
  const who = () => (target() ? nameOf(target()) : 'a worker')
  const facts = { label: verb.replace(/_/g, ' '), detail: null, mono: false }
  switch (verb) {
    case 'harnesses':
      facts.label = ask ? 'Look up the harnesses' : 'Looked up the harnesses'
      break
    case 'spawn': {
      const harness = args.harness ? harnessName(args.harness) : 'a'
      const title = firstLine(args.title) || cut(firstLine(args.brief), 60)
      facts.label = `${ask ? 'Start' : 'Started'} ${harness === 'a' ? 'a' : `a ${harness}`} worker${title ? `: ${title}` : ''}`
      if (typeof args.brief === 'string' && args.brief.trim()) facts.detail = args.brief.trim()
      break
    }
    case 'list':
      facts.label = ask ? 'Check the team' : 'Checked the team'
      break
    case 'read':
      facts.label = `Read ${who()}`
      if (Number.isFinite(args.tail)) facts.detail = `its last ${args.tail} message${args.tail === 1 ? '' : 's'}`
      break
    case 'wait': {
      if (result && typeof result === 'string') {
        const from = [...result.matchAll(/Report from your worker "([^"]*)"/g)].map((m) => m[1])
        if (from.length) {
          facts.label = from.length === 1 ? `Report from ${from[0]}` : `Reports from ${from.slice(0, 2).join(' and ')}${from.length > 2 ? ` and ${from.length - 2} more` : ''}`
          break
        }
        if (/Note from your worker/.test(result)) {
          facts.label = 'A note from a worker'
          break
        }
      }
      const n = runningCount(sessions, chatId)
      facts.label = n > 0 ? `Waiting for ${n} worker${n === 1 ? '' : 's'}` : 'Waiting for workers'
      break
    }
    case 'report': {
      const lead = leadOf(sessions, sessions.find((s) => s.id === chatId))
      facts.label = lead ? `${ask ? 'Send a note to' : 'Note to'} ${nameOf(lead)}` : ask ? 'Send a note to the lead' : 'Note to the lead'
      if (typeof args.text === 'string' && args.text.trim()) facts.detail = args.text.trim()
      break
    }
    case 'message':
      facts.label = `${ask ? 'Message' : 'Message to'} ${who()}${args.mode === 'steer' ? ' (into its running turn)' : ''}`
      if (typeof args.text === 'string' && args.text.trim()) facts.detail = args.text.trim()
      break
    case 'stop':
      facts.label = `${ask ? 'Stop' : 'Stopped'} ${who()}`
      break
    default:
      break
  }
  return facts
}

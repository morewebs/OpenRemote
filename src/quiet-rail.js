// The chat shows the conversation. Thinking gets a quiet row of its own;
// consecutive tools share a single row. Pending decisions stay, because
// the person has to answer them.

import { browserTool, foldsResult, isBrowserTool } from './browser-tools.js'
import { commandSummary, isCommandRun, shellInner } from './command-card.js'
import { repairMojibake } from './text-repair.js'
import { canonical } from './status-words.js'

/// Codex names its shell tool after the protocol item it comes from. A person
/// reads that step as the command it ran, never as the protocol's word.
const isCommandExecution = (tool) => tool?.name === 'commandExecution'

/// How one tool call reads in that row. The browser's tools say what they did to a page
/// ("Browser · read page") rather than the MCP name they arrive under, which is the one
/// place a person meets the agent's browser work.
export function toolLabel(tool) {
  if (isCommandExecution(tool)) return 'Ran'
  if (!isBrowserTool(tool?.name)) return tool?.name
  return browserTool(tool.name, tool.input).label
}

/// Consecutive tools become one work row. A thought, message or decision
/// keeps its place in the conversation.
export function quietRail(items) {
  const out = []
  let work = null
  const flush = () => {
    if (!work) return
    out.push(work)
    work = null
  }
  for (const item of items) {
    if (item.kind === 'tool') {
      if (!work) work = { kind: 'work', id: `work-${item.id}`, items: [] }
      work.items.push(item)
      continue
    }
    flush()
    out.push(item)
  }
  flush()
  return out
}

/// One line for a work row. Live names the step that is running. A
/// finished row names up to three tools, or counts them.
export function workLabel(items, live) {
  const tools = items.filter((item) => item.kind === 'tool')
  if (live) {
    const running = tools.find((item) => item.result == null)
    if (running && isCommandExecution(running)) {
      const what = commandSummary(running.input?.command, 56)
      return what ? `Running ${what}` : 'Running a command'
    }
    if (running) return toolLabel(running)
    return 'Working'
  }
  const names = []
  let commands = 0
  for (const tool of tools) {
    if (isCommandExecution(tool)) {
      commands += 1
      continue
    }
    const name = toolLabel(tool)
    if (!names.includes(name)) names.push(name)
  }
  if (commands > 0) names.unshift(commands === 1 ? 'Ran a command' : `Ran ${commands} commands`)
  if (names.length <= 3) return names.join(', ')
  return `${tools.length} steps`
}

/// What one step shows in an opened work row: its name, a one-line hint,
/// the browser's facts in words (what it acted on, where, what it typed or
/// ran - which replace the hint), and its output, which a page read as
/// text (the browser's snapshot) keeps folded until asked for.
export function workStep(item) {
  const browser = isBrowserTool(item?.name) ? browserTool(item.name, item.input) : null
  // A command's output is what the shell printed, and a Windows shell prints a
  // UTF-8 file's words through the wrong code page. Put them back for display,
  // and keep the text as received for the person who wants it.
  const fixed = isCommandRun(item?.input) && typeof item?.result === 'string' ? repairMojibake(item.result) : null
  return {
    label: toolLabel(item),
    hint: browser ? null : toolHint(item),
    browser,
    out: fixed?.repaired ? fixed.text : (item?.result ?? null),
    received: fixed?.repaired ? item.result : null,
    repaired: Boolean(fixed?.repaired),
    fold: Boolean(browser) && foldsResult(item.name),
  }
}

export function workIsLive(items, running) {
  return running && items.some((item) => item.kind === 'tool' && item.result == null)
}

export function reasoningLabel({ live, tokens, durationMs }) {
  if (live) {
    if (!Number.isInteger(tokens) || tokens <= 0) return 'Thinking...'
    return `Thinking... \u00b7 ${tokens.toLocaleString()} ${tokens === 1 ? 'token' : 'tokens'}`
  }
  if (!Number.isFinite(durationMs) || durationMs <= 0) return 'Thought'
  const seconds = Math.floor(durationMs / 1000)
  if (seconds === 0) return 'Thought for <1s'
  if (seconds < 60) return `Thought for ${seconds}s`
  return `Thought for ${Math.floor(seconds / 60)}m ${seconds % 60}s`
}

const WAIT_LABELS = ['Untangling', 'Following the thread', 'Connecting the dots', 'Finding a way in']

// Put the activity on one live work row or at the end of the rail. A reply
// already has the reader's attention, as does an unanswered question.
export function railActivity(items, status, { reachable = true } = {}) {
  const phase = canonical(status)
  if (!reachable || !['starting', 'working'].includes(phase) || items.some((item) => item.kind === 'decision' && item.pending)) return null
  let promptId = ''
  let hasReply = false
  let workId = null
  let reasoningId = null
  let pastWork = false
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i]
    // An interrupted tool can be left without a result in the transcript.
    // It should not light up again when the next prompt starts running.
    if (item.kind === 'message' && item.role === 'user') {
      promptId = item.id ?? ''
      break
    }
    if (item.kind === 'stream') return null
    if (item.kind === 'message' && item.role === 'agent') hasReply = true
    if (hasReply || reasoningId != null || workId != null) continue
    if (item.kind === 'work') {
      pastWork = true
      if (workIsLive(item.items, true)) workId = item.id
    } else if (item.kind === 'reasoning-stream' && !pastWork) {
      reasoningId = item.id
    }
  }
  if (workId != null) return { workId }
  if (phase === 'starting') return { label: 'Starting the App', waitKey: 'starting' }
  if (hasReply && reasoningId == null) return null
  // Use the prompt instead of a ticker or random render so a turn keeps
  // its phrase across stream updates, reopening the chat, and split panes.
  let hash = 0
  for (let i = 0; i < promptId.length; i++) hash = (31 * hash + promptId.charCodeAt(i)) >>> 0
  const wait = { label: WAIT_LABELS[hash % WAIT_LABELS.length], waitKey: `reply-${promptId}` }
  return reasoningId == null ? wait : { ...wait, reasoningId }
}

/// One short hint for an expanded step. Never the JSON body.
export function toolHint(item) {
  const input = item.input
  if (!input || typeof input !== 'object') return null
  if (typeof input.url === 'string') return urlLabel(input.url)
  if (typeof input.file_path === 'string') return input.file_path
  if (typeof input.command === 'string') return shellInner(input.command)
  if (typeof input.id === 'string' && input.id.length < 80) return input.id
  if (typeof input.query === 'string') return clip(input.query)
  if (typeof input.code === 'string') return clip(input.code)
  return null
}

function urlLabel(href) {
  try {
    const url = new URL(href)
    const path = url.pathname === '/' ? '' : url.pathname
    return `${url.host}${path}`
  } catch {
    return href
  }
}

function clip(text) {
  const line = text.split('\n')[0].trim()
  if (line.length <= 72) return line
  return `${line.slice(0, 71)}…`
}

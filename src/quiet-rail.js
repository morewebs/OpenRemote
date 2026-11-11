// The chat shows the conversation. Thinking and tool traces are one
// quiet row, not a log of every call. Pending decisions stay, because
// the person has to answer them.

import { browserTool, foldsResult, isBrowserTool } from './browser-tools.js'

/// How one tool call reads in that row. The browser's tools say what they did to a page
/// ("Browser · read page") rather than the MCP name they arrive under, which is the one
/// place a person meets the agent's browser work.
export function toolLabel(tool) {
  if (!isBrowserTool(tool?.name)) return tool?.name
  return browserTool(tool.name, tool.input).label
}

/// Consecutive reasoning and tool items become one work row. Everything
/// the person reads (their message, the reply, a note, a question that
/// is still waiting) stays where it is.
export function quietRail(items) {
  const out = []
  let work = null
  const flush = () => {
    if (!work) return
    out.push(work)
    work = null
  }
  for (const item of items) {
    if (item.kind === 'reasoning' || item.kind === 'reasoning-stream' || item.kind === 'tool') {
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
  const thinking = items.some((item) => item.kind === 'reasoning' || item.kind === 'reasoning-stream')
  if (live) {
    const running = tools.find((item) => item.result == null)
    if (running) return toolLabel(running)
    if (thinking) return 'Thinking…'
    return 'Working…'
  }
  if (tools.length === 0) return 'Thought'
  const names = []
  for (const tool of tools) {
    const name = toolLabel(tool)
    if (!names.includes(name)) names.push(name)
  }
  if (names.length <= 3) return names.join(', ')
  return `${tools.length} steps`
}

/// What one step shows in an opened work row: its name, a one-line hint,
/// the browser's facts in words (what it acted on, where, what it typed or
/// ran - which replace the hint), and its output, which a page read as
/// text (the browser's snapshot) keeps folded until asked for.
export function workStep(item) {
  const browser = isBrowserTool(item?.name) ? browserTool(item.name, item.input) : null
  return {
    label: toolLabel(item),
    hint: browser ? null : toolHint(item),
    browser,
    out: item?.result ?? null,
    fold: Boolean(browser) && foldsResult(item.name),
  }
}

export function workIsLive(items, running) {
  if (!running) return false
  return items.some(
    (item) => item.kind === 'reasoning-stream' || (item.kind === 'tool' && item.result == null),
  )
}

/// One short hint for an expanded step. Never the JSON body.
export function toolHint(item) {
  const input = item.input
  if (!input || typeof input !== 'object') return null
  if (typeof input.url === 'string') return urlLabel(input.url)
  if (typeof input.file_path === 'string') return input.file_path
  if (typeof input.command === 'string') return input.command
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

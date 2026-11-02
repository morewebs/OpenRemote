// A chat as Markdown: the transcript the console already holds, written out
// so it can be pasted into a note, an issue or a message.
//
// The transcript's items are a small vocabulary (message, reasoning, stream,
// tool, decision), and each one earns its own shape here: who spoke, what a
// tool was asked and what it answered, what a decision asked and how it was
// answered. Two kinds of honesty decide most of the rules: nothing is
// invented for an item that carries no text (no empty "**Claude**" with
// nothing under it), and a tool's own output is left exactly as the harness
// wrote it, fenced as code, never reflowed or syntax-guessed.
//
// Reasoning is left out on purpose. It is the model's thinking, not what was
// said, and it is the one part of the transcript a reader never quotes.

import { decisionText } from './question-line.js'
import { harnessLabel } from './harness-names.js'
import { chatTitle } from './shape.js'

// One tool's input or output is capped: a chat that pasted a whole log should
// copy as something a person can paste. The cap is generous enough for a
// useful result and small enough that one runaway tool cannot make the copy
// unusable; the marker under it says how much was left out, and the answer
// line counts how many sections were shortened. Inputs are capped too: a
// single large write is as long as any output, and a copy is only useful if
// the whole of it is.
export const TOOL_CAP = 4000

// The fence a block needs: one backtick more than the longest run inside it,
// so a tool that prints Markdown (a diff of a README, say) cannot end the
// block early. Three is the minimum a Markdown reader expects.
export function fenceFor(text, min = 3) {
  const longest = (String(text ?? '').match(/`+/g) ?? []).reduce((best, run) => Math.max(best, run.length), 0)
  return '`'.repeat(Math.max(min, longest + 1))
}

// A count a person reads at a glance. Written here rather than through the
// locale so the same chat answers the same way on every machine, and so the
// tests can pin the exact word.
export function thousands(n) {
  const digits = String(Math.max(0, Math.trunc(Number(n) || 0)))
  return digits.replace(/\B(?=(\d{3})+(?!\d))/g, ',')
}

const text = (value) => (typeof value === 'string' && value.trim() ? value : '')

// The fenced block for one tool input or output, capped with a marker that
// says what was left out. `shortened` comes back so the caller can count
// sections; `lang` is the fence's own language tag.
function fenced(body, lang = '') {
  const whole = String(body ?? '')
  if (whole.length <= TOOL_CAP) {
    const fence = fenceFor(whole)
    return { block: `${fence}${lang}\n${whole}\n${fence}`, shortened: false }
  }
  const kept = whole.slice(0, TOOL_CAP)
  const fence = fenceFor(kept)
  const marker = `[... truncated: ${thousands(whole.length)} characters total, ${thousands(TOOL_CAP)} kept ...]`
  return { block: `${fence}${lang}\n${kept}\n${marker}\n${fence}`, shortened: true }
}

// A tool's input, as the harness gave it: the command when there is one
// (shell, so it is fenced as such), otherwise the JSON it sent.
function toolInput(input) {
  if (input == null) return null
  if (typeof input === 'string') return { body: input, lang: '' }
  const command = text(input.command)
  if (command) return { body: command, lang: 'sh' }
  try {
    return { body: JSON.stringify(input, null, 2), lang: 'json' }
  } catch {
    return null
  }
}

function toolItem(item, out) {
  const name = text(item.name) || 'a tool'
  const failure = item.state === 'failed'
  const input = toolInput(item.input)
  const result = typeof item.result === 'string' && item.result.length > 0 ? item.result : ''
  // No input and no result is a call the transcript knows nothing about:
  // naming the tool alone would invent a call that was never shown.
  if (!input && !result && !failure) return false
  out.push(`**Tool: ${name}${failure ? ' (failed)' : ''}**`, '')
  if (input) {
    // The input is capped by the same rule as the output: a write of one big
    // file would otherwise make the copy as long as the file.
    const { block, shortened } = fenced(input.body, input.lang)
    out.push(block, '')
    if (shortened) out.shortened += 1
  }
  if (result) {
    const { block, shortened } = fenced(result)
    out.push('Result:', '', block, '')
    if (shortened) out.shortened += 1
  }
  out.push('')
  return true
}

function decisionItem(item, out) {
  const { text: words } = decisionText({ harness_request: item.input, tool_name: item.toolName })
  const answered = !item.pending && item.answeredChoice != null
  out.push('**Decision**')
  if (words) out.push('', words)
  if (answered) out.push('', `**Answered:** ${item.answeredChoice}`)
  else if (item.pending) out.push('', '**Waiting for an answer.**')
  out.push('')
  return true
}

/**
 * The whole chat as Markdown, or null when there is nothing to write.
 *
 * Returns `{ text, lines, shortened }`: the markdown, how many lines it is
 * (what the palette answers with), and how many tool sections were cut to
 * the cap. `harness` names the speaker for the harness's own turns;
 * `harnesses` is the daemon's list, which names a harness whose id this
 * build has no name for - so a chat on a plugin's harness is signed with
 * the name that plugin ships, not with its raw id.
 */
export function chatMarkdown(chat, { harness = null, harnesses = null } = {}) {
  const items = Array.isArray(chat?.timeline) ? chat.timeline : []
  const out = []
  out.shortened = 0
  const speaker = harnessLabel(harness ?? chat?.harness ?? null, harnesses) || 'The app'
  let wrote = false
  for (const item of items) {
    if (!item || typeof item !== 'object') continue
    if (item.kind === 'message') {
      const body = text(item.text)
      if (!body) continue
      if (item.role === 'note') out.push(`> ${body.replace(/\n/g, '\n> ')}`)
      else out.push(`**${item.role === 'user' ? 'You' : speaker}**`, '', body)
      out.push('')
      wrote = true
    } else if (item.kind === 'stream') {
      // An assistant message still arriving: its text is what has been said
      // so far, and leaving it out would drop the last thing the chat shows.
      const body = text(item.text)
      if (!body) continue
      out.push(`**${speaker}**`, '', body, '')
      wrote = true
    } else if (item.kind === 'tool') {
      wrote = toolItem(item, out) || wrote
    } else if (item.kind === 'decision') {
      wrote = decisionItem(item, out) || wrote
    }
    // reasoning and reasoning-stream are left out on purpose (see the top).
  }
  if (!wrote) return null
  const body = out.join('\n').replace(/\n{3,}/g, '\n\n').trimEnd()
  // The heading is the same name the sidebar and the header print, from the
  // same reader, so the copy and the window cannot disagree about what a chat
  // is called.
  const title = chatTitle(chat)
  const rendered = `# ${title}\n\n${body}\n`
  return {
    text: rendered,
    lines: rendered.split('\n').length - 1,
    shortened: out.shortened,
  }
}

/** The one line the palette answers with after a copy. */
export function copiedLine({ lines = 0, shortened = 0 } = {}) {
  const body = `Copied ${thousands(lines)} ${lines === 1 ? 'line' : 'lines'}`
  if (!shortened) return `${body}.`
  return `${body} (${shortened} ${shortened === 1 ? 'section' : 'sections'} shortened).`
}

/// Whether this window can copy a chat at all, so both the palette's row and
/// the sidebar's own menu ask one question. The copy reads this window's own
/// record and fetches nothing, so a chat with no transcript here has nothing to
/// give, and a chat whose only turns render to nothing is the same case.
export function canCopyChat(chat) {
  if (!Array.isArray(chat?.timeline) || chat.timeline.length === 0) return false
  return chatMarkdown(chat) !== null
}

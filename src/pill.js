// The pill's brain: turn a sentence into
// either a rule draft (action card) or a plain reply. Deliberately dumb -
// keyword shapes, not NLP. The real product routes this through an agent
// behind the same card; the interaction is what we're testing.
//
// Adapted to the real console: harness ids are the daemon's own, devices
// are machines, projects are workspace folder names.

import { whenSentence } from './automation.js'

const HARNESS_WORDS = [
  ['claude code', 'claude'],
  ['claude', 'claude'],
  ['codex', 'codex'],
  ['gpt', 'codex'],
  ['opencode', 'opencode'],
  ['pi agent', 'pi'],
  ['pi', 'pi'],
  ['grok', 'grok'],
  ['antigravity', 'agy'],
  ['agy', 'agy'],
]

const BRANCH_WORDS = ['main', 'master', 'develop', 'staging', 'trunk']

export function parsePill(text, ctx) {
  const raw = String(text ?? '').trim()
  const t = raw.toLowerCase()
  const machines = ctx?.machines ?? []
  const defaults = ctx?.defaults ?? {}
  const projects = ctx?.projects ?? []

  const wantsRule = /\bwhen\b/.test(t) || /^(make|create|add|set up|new)\b/.test(t)
  if (!wantsRule) {
    return {
      reply:
        'Describe when it should fire and what the chat should do - e.g. "When the api pipeline on main fails, open a chat to bisect the flaky test."',
    }
  }

  let kind = 'pipeline'
  if (/sentry|unresolved issue|errors? spike/.test(t)) kind = 'errors'
  else if (/webhook/.test(t)) kind = 'webhook'
  else if (/schedule|nightly|daily|every (day|morning|weekday)/.test(t)) kind = 'schedule'
  else if (/review/.test(t)) kind = 'review'
  else if (/release|tag/.test(t)) kind = 'release'

  let project = null
  for (const p of projects) {
    if (new RegExp(`\\b${p}\\b`).test(t)) {
      project = p
      break
    }
  }

  // A waiting machine is not a place a rule can run. Naming one in local
  // mode falls through to this computer instead of drafting a check-in.
  let machineId = null
  for (const m of machines) {
    if (m.status !== 'online') continue
    if (t.includes(m.name.toLowerCase()) || t.includes(m.id.toLowerCase())) {
      machineId = m.id
      break
    }
  }

  let branch = 'main'
  for (const b of BRANCH_WORDS) {
    if (new RegExp(`\\bon ${b}\\b`).test(t)) {
      branch = b
      break
    }
  }

  let time = '09:00'
  const tm = t.match(/\b(\d{1,2}:\d{2})\b/)
  if (tm) time = tm[1].padStart(5, '0')

  let harness = null
  for (const [word, id] of HARNESS_WORDS) {
    if (t.includes(word)) {
      harness = id
      break
    }
  }

  let task = raw
  // "open a chat [with codex] [on the server] to …" - lazy match to the
  // first to/that/which so routing fragments stay out of the task
  const taskMatch = raw.match(/open (?:a |an )?chat\b.*?\b(?:to|that|which)\b\s*(.+)$/i)
  if (taskMatch) task = taskMatch[1]
  else {
    const thenMatch = raw.match(/\bthen\s+(.+)$/i)
    if (thenMatch) task = thenMatch[1]
  }
  task = task.trim().replace(/[.!?]+$/, '')

  const when = whenSentence({ kind, time })

  return {
    draft: {
      name: when,
      trigger: { kind, time, project, branch },
      when,
      harness: harness ?? defaults.harness ?? 'claude',
      machineId:
        machineId ?? defaults.machineId ?? machines.find((m) => m.status === 'online')?.id ?? machines[0]?.id,
      project: project ?? defaults.project ?? null,
      task,
    },
  }
}

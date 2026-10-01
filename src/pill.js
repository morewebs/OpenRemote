// The pill's brain (prototype edition): turn a sentence into either
// a rule draft (action card) or a plain reply. Deliberately dumb —
// keyword shapes, not NLP. The real product routes this through an
// agent behind the same card; the interaction is what we're testing.

import { normalizeTrigger, whenSentence } from './automation.js'

const HARNESS_WORDS = [
  ['claude code', 'cc'],
  ['claude', 'cc'],
  ['codex', 'cx'],
  ['gpt', 'cx'],
  ['opencode', 'od'],
  ['pi agent', 'pi'],
  ['pi', 'pi'],
  ['amp', 'amp'],
  ['gemini', 'gem'],
  ['grok', 'grok'],
  ['cursor', 'cur'],
]

const BRANCH_WORDS = ['main', 'master', 'develop', 'staging', 'trunk']

export function parsePill(text, ctx) {
  const raw = String(text ?? '').trim()
  const t = raw.toLowerCase()
  const devices = ctx?.devices ?? []
  const defaults = ctx?.defaults ?? {}
  const projects = ctx?.projects ?? []

  const wantsRule = /\bwhen\b/.test(t) || /^(make|create|add|set up|new)\b/.test(t)
  if (!wantsRule) {
    return {
      reply:
        'Describe when it should fire and what the chat should do — e.g. “When the api pipeline on main fails, open a chat to bisect the flaky test.”',
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
    if (new RegExp(`\\b${p.id}\\b`).test(t)) {
      project = p.id
      break
    }
  }

  let deviceId = null
  for (const d of devices) {
    if (t.includes(d.name.toLowerCase()) || t.includes(d.id.toLowerCase())) {
      deviceId = d.id
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
  if (tm) time = tm[1]

  let harness = null
  for (const [word, id] of HARNESS_WORDS) {
    if (t.includes(word)) {
      harness = id
      break
    }
  }

  let task = raw
  // "open a chat [with codex] [on the server] to …" — lazy match to the
  // first to/that/which so routing fragments stay out of the task
  const taskMatch = raw.match(/open (?:a |an )?chat\b.*?\b(?:to|that|which)\b\s*(.+)$/i)
  if (taskMatch) task = taskMatch[1]
  else {
    const thenMatch = raw.match(/\bthen\s+(.+)$/i)
    if (thenMatch) task = thenMatch[1]
  }
  task = task.trim().replace(/[.!?]+$/, '')

  const trigger = normalizeTrigger({
    kind,
    project: project ?? defaults.project ?? 'webapp',
    branch,
    time,
  })
  const when = whenSentence(trigger)

  return {
    draft: {
      name: when,
      trigger,
      when,
      harness: harness ?? defaults.harness ?? 'cc',
      deviceId:
        deviceId ?? defaults.deviceId ?? devices.find((d) => d.status === 'online')?.id ?? devices[0]?.id,
      task,
    },
  }
}

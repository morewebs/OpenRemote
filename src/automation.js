const KINDS = ['pipeline', 'errors', 'webhook', 'schedule', 'review', 'release']

function titled(project) {
  const name = String(project || 'webapp')
  return name.charAt(0).toUpperCase() + name.slice(1)
}

export function normalizeTrigger(trigger) {
  const kind = KINDS.includes(trigger?.kind) ? trigger.kind : 'pipeline'
  return {
    kind,
    project: String(trigger?.project || 'webapp'),
    branch: String(trigger?.branch || 'main').trim() || 'main',
    time: /^\d{2}:\d{2}$/.test(trigger?.time ?? '') ? trigger.time : '09:00',
  }
}

export function whenSentence(trigger) {
  const t = normalizeTrigger(trigger)
  if (t.kind === 'pipeline') return `The ${t.project} pipeline on ${t.branch} fails`
  if (t.kind === 'errors') return `A new unresolved issue is assigned in ${t.project}`
  if (t.kind === 'webhook') return `${titled(t.project)} rejects a webhook signature`
  if (t.kind === 'schedule') return `Weekdays at ${t.time}`
  if (t.kind === 'review') return `Someone requests a review in ${t.project}`
  return `A v* tag is pushed in ${t.project}`
}

export function inferTrigger(rule) {
  if (rule?.trigger?.kind) return normalizeTrigger({ ...rule.trigger, project: rule.trigger.project || rule.project })
  const when = String(rule?.when ?? '')
  const pipeline = when.match(/^The (\w+) pipeline on (\S+) fails$/)
  if (pipeline) return normalizeTrigger({ kind: 'pipeline', project: pipeline[1], branch: pipeline[2] })
  const errors = when.match(/^A new unresolved issue is assigned in (\w+)$/)
  if (errors) return normalizeTrigger({ kind: 'errors', project: errors[1] })
  const webhook = when.match(/^(\w+) rejects a webhook signature$/)
  if (webhook) return normalizeTrigger({ kind: 'webhook', project: webhook[1].toLowerCase() })
  const schedule = when.match(/^Weekdays at (\d{2}:\d{2})$/)
  if (schedule) return normalizeTrigger({ kind: 'schedule', time: schedule[1], project: rule?.project })
  const review = when.match(/^Someone requests a review in (\w+)$/)
  if (review) return normalizeTrigger({ kind: 'review', project: review[1] })
  const release = when.match(/^A v\* tag is pushed in (\w+)$/)
  if (release) return normalizeTrigger({ kind: 'release', project: release[1] })
  return normalizeTrigger({ kind: 'pipeline', project: rule?.project, branch: 'main' })
}

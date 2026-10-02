// Automations: rules that open a chat when their trigger fires. The
// clock and a webhook are the real sources today; Run now is the same
// path by hand. The pill at the bottom drafts rules from a sentence —
// the prototype's keyword brain behind the same action card.

import { useMemo, useState } from 'react'
import { ArrowUpRight, Check, Lightning, PencilSimple, Play, Plus, TrashSimple } from '@phosphor-icons/react'
import AutomationForm from './AutomationForm.jsx'
import PillComposer from './PillComposer.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { workspaceName } from './state/reducer.js'
import { whenSentence, CONNECTOR_KINDS } from './automation.js'
import { parsePill } from './pill.js'
import './devices.css'
import './automations.css'
import './pill.css'

const STARTERS = [
  {
    id: 'deps',
    name: 'Nightly dependency audit',
    time: '09:00',
    task: 'Check outdated dependencies and list the ones that are safe to bump.',
  },
]

const RAN = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })

function ranLabel(rule) {
  if (!rule.last_run) return 'never ran'
  return `ran ${RAN.format(new Date(rule.last_run))}`
}

export default function AutomationsView({ onOpenChat }) {
  const {
    api,
    automations,
    capabilities,
    machines,
    sessions,
    saveAutomation,
    removeAutomation,
    enableAutomation,
    runAutomation,
  } = useConsole()
  const [editing, setEditing] = useState(null) // null | {} (new) | rule
  const [busyId, setBusyId] = useState(null)
  const [error, setError] = useState(null)
  // The pill's conversation: {role: user|reply|card|done|note, text?, draft?}
  const [thread, setThread] = useState([])

  const rules = automations ?? []
  const availableIds = new Set(
    (capabilities?.harnesses ?? []).filter((h) => h.available).map((h) => h.id),
  )
  const machineName = (id) => (machines ?? []).find((m) => m.machine.id === id)?.machine.name ?? id

  // The pill's context: machines, workspace folders, and the defaults a
  // draft falls back to.
  const workspaces = useMemo(() => {
    const seen = new Map()
    for (const s of sessions ?? []) seen.set(workspaceName(s.workspace), s.workspace)
    try {
      const recents = JSON.parse(localStorage.getItem('openremote-recent-workspaces') ?? '[]')
      for (const path of recents) seen.set(workspaceName(path), path)
    } catch {
      /* storage unavailable */
    }
    return [...seen.entries()] // [name, path]
  }, [sessions])
  const thisMachine = (machines ?? []).find((m) => m.machine.this_machine)

  const run = async (action, id, ...args) => {
    if (busyId) return
    setBusyId(id ?? 'view')
    setError(null)
    try {
      return await action(...args)
    } catch (err) {
      setError(err.message ?? String(err))
      return null
    } finally {
      setBusyId(null)
    }
  }

  const runNow = async (rule) => {
    const session = await run(() => runAutomation(rule.id), rule.id)
    if (session) onOpenChat?.(session.id)
  }

  const addStarter = async (starter) => {
    const [, path] = workspaces[0] ?? [null, null]
    if (!path) {
      // No workspace to run it in — the form is the honest next step.
      setEditing({
        name: starter.name,
        trigger: { kind: 'schedule', time: starter.time },
        task: starter.task,
      })
      return
    }
    await run(
      () =>
        saveAutomation({
          name: starter.name,
          trigger: { kind: 'schedule', time: starter.time },
          harness: 'claude',
          workspace: path,
          machine: thisMachine?.machine.id,
          task: starter.task,
        }),
      starter.id,
    )
  }

  // ---- the pill ----

  const pillContext = {
    machines: (machines ?? []).map((m) => ({
      id: m.machine.id,
      name: m.machine.name,
      status: m.machine.status,
    })),
    projects: workspaces.map(([name]) => name),
    defaults: {
      harness: (capabilities?.harnesses ?? []).find((h) => h.available)?.id ?? 'claude',
      machineId: thisMachine?.machine.id,
    },
  }

  const submitPill = async (text) => {
    setThread((t) => [...t, { role: 'user', text }])
    const out = parsePill(text, pillContext)
    if (out.reply) {
      setThread((t) => [...t, { role: 'reply', text: out.reply }])
      return
    }
    setThread((t) => [...t, { role: 'card', draft: out.draft, id: crypto.randomUUID() }])
  }

  const createFromDraft = async (draft, cardId) => {
    const path = workspaces.find(([name]) => name === draft.project)?.[1] ?? workspaces[0]?.[1]
    if (!path) {
      setThread((t) => [
        ...t,
        { role: 'note', text: 'No workspace yet — name one in the form first.' },
      ])
      return
    }
    const saved = await run(
      () =>
        saveAutomation({
          name: draft.name,
          trigger: { kind: draft.trigger.kind, time: draft.trigger.time },
          harness: draft.harness,
          workspace: path,
          machine: draft.machineId,
          task: draft.task,
        }),
      cardId,
    )
    if (saved) {
      setThread((t) => [
        ...t.map((item) => (item.id === cardId ? { ...item, consumed: true } : item)),
        { role: 'done', text: 'Rule created · enabled', ruleId: saved.id },
      ])
    }
  }

  const undoCreate = async (ruleId) => {
    await run(() => removeAutomation(ruleId), ruleId)
    setThread((t) => [...t, { role: 'note', text: 'Rule removed.' }])
  }

  const editFromDraft = (draft, cardId) => {
    const path = workspaces.find(([name]) => name === draft.project)?.[1] ?? workspaces[0]?.[1]
    setEditing({
      name: draft.name,
      trigger: { kind: draft.trigger.kind, time: draft.trigger.time },
      harness: draft.harness,
      machine: draft.machineId,
      workspace: path ?? '',
      task: draft.task,
    })
    setThread((t) => [
      ...t.map((item) => (item.id === cardId ? { ...item, consumed: true } : item)),
      { role: 'note', text: 'Opened in the form.' },
    ])
  }

  const enabledCount = rules.filter((r) => r.enabled).length

  return (
    <div className="automations">
      <header className="dv-head">
        <div>
          <h1 className="dv-title">Automations</h1>
          <p className="dv-meta">
            {rules.length} {rules.length === 1 ? 'rule' : 'rules'} · {enabledCount} enabled
          </p>
        </div>
        <button type="button" className="dv-act" onClick={() => setEditing({})}>
          New automation
        </button>
      </header>

      <div className="am-list">
        {rules.length === 0 && (
          <p className="am-empty">No rules yet. Write one, or add a starter below.</p>
        )}
        {rules.map((rule) => {
          const missing = !availableIds.has(rule.harness)
          return (
            <div key={rule.id} className={`am-rule${rule.enabled ? '' : ' off'}`}>
              <div className="am-rule-main">
                <div className="am-rule-body">
                  <div className="am-name">{rule.name}</div>
                  <div className="am-line">
                    {whenSentence(rule.trigger)} · {harnessName(rule.harness)} on{' '}
                    {machineName(rule.machine)} · {ranLabel(rule)}
                  </div>
                  {rule.trigger.kind === 'webhook' && api?.url && (
                    <code className="am-hook">
                      {api.url}/hooks/{rule.id}?key={rule.trigger.key}
                    </code>
                  )}
                </div>
                <div className="am-actions">
                  {rule.last_chat && (
                    <button
                      className="am-icon"
                      title="Open the last chat"
                      onClick={() => onOpenChat?.(rule.last_chat)}
                    >
                      <ArrowUpRight size={13} />
                    </button>
                  )}
                  <button
                    className="am-icon"
                    title="Run now"
                    disabled={!rule.enabled || busyId != null}
                    onClick={() => runNow(rule)}
                  >
                    <Play size={13} weight="fill" />
                  </button>
                  <button className="am-icon" title="Edit" onClick={() => setEditing(rule)}>
                    <PencilSimple size={13} />
                  </button>
                  <button
                    className="am-icon"
                    title="Remove"
                    onClick={() => run(() => removeAutomation(rule.id), rule.id)}
                  >
                    <TrashSimple size={13} />
                  </button>
                  <button
                    className={`am-switch${rule.enabled ? ' on' : ''}`}
                    role="switch"
                    aria-checked={rule.enabled}
                    aria-label={`${rule.name} enabled`}
                    onClick={() => run(() => enableAutomation(rule.id, !rule.enabled), rule.id)}
                  />
                </div>
              </div>
              {rule.enabled && missing && (
                <p className="am-note">
                  {harnessName(rule.harness)} is not installed on{' '}
                  {machineName(rule.machine)}.
                </p>
              )}
            </div>
          )
        })}
      </div>

      <div className="am-starters">
        <div className="am-starters-title">Starters</div>
        {STARTERS.map((starter) => {
          const added = rules.some((r) => r.name === starter.name)
          return (
            <div key={starter.id} className="am-starter">
              <div>
                <div className="am-name">{starter.name}</div>
                <div className="am-line">
                  Every day at {starter.time} · {harnessName('claude')}
                </div>
              </div>
              <button
                type="button"
                className="am-icon"
                disabled={busyId != null}
                onClick={() => addStarter(starter)}
                title={added ? 'Added' : 'Add'}
              >
                {added ? <Check size={13} /> : <Plus size={13} />}
              </button>
            </div>
          )
        })}
      </div>

      {error && <p className="am-error">{error}</p>}

      {editing != null && (
        <AutomationForm rule={editing} onClose={() => setEditing(null)} />
      )}

      <div className="pill-dock">
        <div className="pt-stack">
          {thread.map((item, index) => {
            if (item.role === 'user') {
              return (
                <div key={index} className="pt-user">
                  {item.text}
                </div>
              )
            }
            if (item.role === 'reply' || item.role === 'note') {
              return (
                <p key={index} className="pt-reply">
                  {item.text}
                </p>
              )
            }
            if (item.role === 'done') {
              return (
                <p key={index} className="pt-done">
                  <Check size={12} weight="bold" /> {item.text}{' '}
                  <button className="pt-undo" onClick={() => undoCreate(item.ruleId)}>
                    Undo
                  </button>
                </p>
              )
            }
            // the draft action card
            const draft = item.draft
            const connectorKind = CONNECTOR_KINDS.includes(draft.trigger.kind)
            const hookKind = draft.trigger.kind === 'webhook'
            return (
              <div key={item.id} className="pt-card">
                <div className="pt-card-head">
                  <Lightning size={11} weight="fill" /> CREATE RULE
                </div>
                <div className="pt-card-name">{draft.name}</div>
                <div className="pt-card-chips">
                  <span className="pt-chip">{harnessName(draft.harness)}</span>
                  <span className="pt-chip">{machineName(draft.machineId)}</span>
                  <span className="pt-chip">
                    {hookKind ? 'webhook' : `every day at ${draft.trigger.time}`}
                  </span>
                </div>
                <div className="pt-card-task">“{draft.task}”</div>
                {connectorKind && (
                  <p className="pt-card-note">
                    That trigger needs a connector that isn’t built yet — a schedule or a webhook
                    fires today.
                  </p>
                )}
                {!item.consumed && (
                  <div className="pt-card-actions">
                    <button
                      className="pt-primary"
                      disabled={connectorKind || busyId != null}
                      onClick={() => createFromDraft(draft, item.id)}
                    >
                      Create rule
                    </button>
                    <button
                      disabled={connectorKind}
                      onClick={() => editFromDraft(draft, item.id)}
                    >
                      Edit in form
                    </button>
                    <button
                      className="pt-quiet"
                      onClick={() =>
                        setThread((t) =>
                          t.map((x) => (x.id === item.id ? { ...x, consumed: true } : x)),
                        )
                      }
                    >
                      Cancel
                    </button>
                  </div>
                )}
              </div>
            )
          })}
        </div>
        <PillComposer onSubmit={submitPill} />
      </div>
    </div>
  )
}

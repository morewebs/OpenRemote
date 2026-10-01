import { useState } from 'react'
import { ArrowUpRight, Check, PencilSimple, Play, Plus, TrashSimple, X } from '@phosphor-icons/react'
import { AUTOMATION_STARTERS } from './library.js'
import { MODELS, harnessName, modelName } from './catalog.js'
import { PROJECTS, taskPlacement } from './world.js'
import { whenSentence } from './automation.js'
import { parsePill } from './pill.js'
import AutomationForm from './AutomationForm.jsx'
import PillComposer from './PillComposer.jsx'
import './devices.css'
import './panels.css'
import './automations.css'

function deviceName(devices, id) {
  return devices.find((d) => d.id === id)?.name ?? id
}

function whenLabel(at) {
  if (!at) return 'never ran'
  return `ran ${new Date(at).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}`
}

export default function AutomationsView({ world, harnesses, onToggle, onRun, onOpenChat, onAdd, onUpdate, onRemove, onStarter }) {
  const [form, setForm] = useState(null)
  const [thread, setThread] = useState([])
  const added = new Set(world.automations.map((a) => a.starterId).filter(Boolean))
  const enabledCount = world.automations.filter((a) => a.enabled).length

  const patchExchange = (id, patch) =>
    setThread((th) => th.map((x) => (x.id === id ? { ...x, ...patch } : x)))

  const handlePill = (text) => {
    const res = parsePill(text, {
      devices: world.devices,
      defaults: world.defaults,
      projects: PROJECTS,
    })
    setThread((th) => [
      ...th,
      { id: crypto.randomUUID(), userText: text, ...res, status: res.draft ? 'pending' : 'reply' },
    ])
  }

  const confirmCard = (x) => {
    const id = crypto.randomUUID()
    const modelId = (MODELS[x.draft.harness] ?? [])[0]?.id
    onAdd({
      id,
      name: x.draft.name,
      trigger: x.draft.trigger,
      harness: x.draft.harness,
      model: modelId,
      modelLabel: modelName(x.draft.harness, modelId),
      deviceId: x.draft.deviceId,
      project: x.draft.trigger.project,
      task: x.draft.task,
      enabled: true,
    })
    patchExchange(x.id, { status: 'done', ruleId: id })
  }

  return (
    <div className="panel">
      <header className="dv-head pn-head">
        <div>
          <h1 className="dv-title">Automations</h1>
          <p className="dv-meta">
            {world.automations.length} {world.automations.length === 1 ? 'rule' : 'rules'} · {enabledCount} enabled
          </p>
        </div>
        <button type="button" className="dv-act" onClick={() => setForm({ rule: null })}>New automation</button>
      </header>

      <h2 className="am-section">Rules</h2>
      {world.automations.length === 0 && (
        <p className="pn-empty">No rules yet. Write one, or add a starter below.</p>
      )}
      <div className="am-list">
        {world.automations.map((a) => {
          const place = taskPlacement(world, { harness: a.harness, deviceId: a.deviceId, mode: 'cloud' })
          return (
            <div key={a.id} className={`am-row${a.enabled ? '' : ' off'}`}>
              <div className="am-row-main">
                <div className="am-text">
                  <div className="am-name">{a.name}</div>
                  <div className="am-sub">
                    {a.when} · {harnessName(a.harness)} on {deviceName(world.devices, a.deviceId)} · {whenLabel(a.lastAt)}
                  </div>
                </div>
                <div className="am-row-actions">
                  {a.lastChatId && (
                    <button
                      type="button"
                      className="am-act"
                      title="Open last chat"
                      onClick={() => onOpenChat(a.lastChatId)}
                    >
                      <ArrowUpRight size={14} />
                    </button>
                  )}
                  <button
                    type="button"
                    className="am-act"
                    title={a.enabled ? 'Run now' : 'Enable the rule to run it'}
                    disabled={!a.enabled}
                    onClick={() => onRun(a.id)}
                  >
                    <Play size={13} weight="fill" />
                  </button>
                  <button type="button" className="am-act" title="Edit" onClick={() => setForm({ rule: a })}>
                    <PencilSimple size={14} />
                  </button>
                  <button type="button" className="am-act" title="Remove" onClick={() => onRemove(a.id)}>
                    <TrashSimple size={14} />
                  </button>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={a.enabled}
                  aria-label={`${a.enabled ? 'Disable' : 'Enable'} ${a.name}`}
                  className={`am-switch${a.enabled ? ' on' : ''}`}
                  onClick={() => onToggle(a.id)}
                >
                  <span className="am-knob" />
                </button>
              </div>
              {a.enabled && place.note && <p className="am-note">{place.note}</p>}
            </div>
          )
        })}
      </div>

      <h2 className="am-section">Starters</h2>
      <div className="am-list">
        {AUTOMATION_STARTERS.map((s) => {
          const has = added.has(s.id)
          return (
            <div key={s.id} className={`am-row am-starter${has ? ' added' : ''}`}>
              <div className="am-row-main">
                <div className="am-text">
                  <div className="am-name">{s.name}</div>
                  <div className="am-sub">{whenSentence(s.trigger)}</div>
                </div>
                {has ? (
                  <span className="am-added">
                    <Check size={12} weight="bold" />
                    Added
                  </span>
                ) : (
                  <button type="button" className="am-add" onClick={() => onStarter(s.id)}>
                    <Plus size={12} weight="bold" />
                    Add
                  </button>
                )}
              </div>
            </div>
          )
        })}
      </div>

      {/* the pill dock: the conversation thread + the everywhere-pill,
          parked at the pane's bottom, above any open popup */}
      <div className="pill-dock">
        {thread.length > 0 && (
          <div className="pt-stack">
            {thread.map((x) => (
              <div key={x.id} className="pt-group">
                <div className="pt-user">{x.userText}</div>

                {x.status === 'reply' && <p className="pt-reply">{x.reply}</p>}

                {x.status === 'pending' && x.draft && (
                  <div className="pt-card">
                    <div className="pt-card-head">Create rule</div>
                    <div className="pt-card-name">{x.draft.name}</div>
                    <div className="pt-card-row">
                      <span className="pt-chip">{harnessName(x.draft.harness)}</span>
                      <span className="pt-chip">{deviceName(world.devices, x.draft.deviceId)}</span>
                      <span className="pt-chip">
                        {x.draft.trigger.kind === 'schedule'
                          ? x.draft.trigger.time
                          : x.draft.trigger.kind === 'pipeline'
                            ? x.draft.trigger.branch
                            : PROJECTS.find((p) => p.id === x.draft.trigger.project)?.name ?? x.draft.trigger.project}
                      </span>
                    </div>
                    <p className="pt-card-task">“{x.draft.task}”</p>
                    <div className="pt-card-actions">
                      <button type="button" className="dv-act pt-primary" onClick={() => confirmCard(x)}>
                        Create rule
                      </button>
                      <button
                        type="button"
                        className="dv-act"
                        onClick={() => {
                          setForm({ rule: null, draft: x.draft })
                          patchExchange(x.id, { status: 'edited' })
                        }}
                      >
                        Edit in form
                      </button>
                      <button type="button" className="dv-act" onClick={() => patchExchange(x.id, { status: 'cancelled' })}>
                        Cancel
                      </button>
                    </div>
                  </div>
                )}

                {x.status === 'done' && (
                  <div className="pt-done">
                    <Check size={13} weight="bold" />
                    Rule created · enabled
                    <button
                      type="button"
                      className="pt-undo"
                      onClick={() => {
                        onRemove(x.ruleId)
                        patchExchange(x.id, { status: 'undone' })
                      }}
                    >
                      Undo
                    </button>
                  </div>
                )}
                {x.status === 'edited' && <p className="pt-note">Opened in the form.</p>}
                {x.status === 'cancelled' && <p className="pt-note">Cancelled.</p>}
                {x.status === 'undone' && <p className="pt-note">Rule removed.</p>}
              </div>
            ))}
          </div>
        )}
        <PillComposer
          placeholder="Ask anything, or describe a rule…"
          onSubmit={handlePill}
        />
      </div>

      {form && (
        <AutomationForm
          key={form.rule?.id ?? `new-${form.seedTask ?? form.draft?.name ?? ''}`}
          rule={form.rule}
          seedTask={form.seedTask}
          draft={form.draft}
          world={world}
          harnesses={harnesses}
          onClose={() => setForm(null)}
          onSave={(input) => {
            if (form.rule) onUpdate(form.rule.id, input)
            else onAdd(input)
            setForm(null)
          }}
        />
      )}
    </div>
  )
}

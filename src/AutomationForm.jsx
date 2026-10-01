import { useEffect, useState } from 'react'
import { CaretDown, X } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { MODELS, harnessName, modelName } from './catalog.js'
import { PROJECTS } from './world.js'
import { inferTrigger, whenSentence } from './automation.js'
import './devices.css'
import './automations.css'

const KINDS = [
  { id: 'pipeline', name: 'Pipeline failed' },
  { id: 'errors', name: 'Errors spiked' },
  { id: 'webhook', name: 'Webhook rejected' },
  { id: 'schedule', name: 'On a schedule' },
  { id: 'review', name: 'Review requested' },
  { id: 'release', name: 'Release tag' },
]

function blank(rule, defaults, seedTask, draft) {
  const trigger =
    draft?.trigger ??
    (rule ? inferTrigger(rule) : { kind: 'pipeline', project: defaults?.project ?? 'webapp', branch: 'main', time: '09:00' })
  return {
    name: rule?.name ?? draft?.name ?? '',
    trigger,
    harness: rule?.harness ?? draft?.harness ?? defaults?.harness ?? 'cc',
    model: rule?.model ?? defaults?.model ?? 'sonnet',
    deviceId: rule?.deviceId ?? draft?.deviceId ?? defaults?.deviceId ?? 'mac-mini',
    task: rule?.task ?? seedTask ?? draft?.task ?? '',
    enabled: rule ? !!rule.enabled : true,
  }
}

export default function AutomationForm({ rule, seedTask, draft, world, harnesses, onClose, onSave }) {
  const [form, setForm] = useState(() => blank(rule, world.defaults, seedTask, draft))
  const [picker, setPicker] = useState(null)
  const set = (patch) => setForm((f) => ({ ...f, ...patch }))
  const trigger = form.trigger
  const setTrigger = (patch) => setForm((f) => ({ ...f, trigger: { ...f.trigger, ...patch } }))
  const modelList = MODELS[form.harness] ?? []
  const model = modelList.find((m) => m.id === form.model) ?? modelList[0]
  const machine = world.devices.find((d) => d.id === form.deviceId)
  const kind = KINDS.find((k) => k.id === trigger.kind) ?? KINDS[0]
  const canSave = form.name.trim().length > 0 && form.task.trim().length > 0

  useEffect(() => {
    const onKey = (e) => {
      if (e.key !== 'Escape' || picker) return
      onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose, picker])

  const openPicker = (kindName) => (e) => setPicker({ kind: kindName, x: e.clientX, y: e.clientY })

  const save = () => {
    if (!canSave) return
    onSave({
      ...form,
      name: form.name.trim(),
      task: form.task.trim(),
      // one project choice: the trigger's project is where the chat
      // lands too — the two were always the same in practice
      project: trigger.project,
      model: model?.id ?? form.model,
      modelLabel: modelName(form.harness, model?.id ?? form.model),
    })
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="dv-modal dv-modal--form af-modal"
        role="dialog"
        aria-modal="true"
        aria-label={rule ? 'Edit automation' : 'New automation'}
      >
        <div className="dv-modal-head af-head">
          <h2 className="dv-modal-title">{rule ? 'Edit automation' : 'New automation'}</h2>
          <div className="af-head-right">
            <button
              type="button"
              role="switch"
              aria-checked={form.enabled}
              aria-label={form.enabled ? 'Enabled' : 'Disabled'}
              className={`am-switch${form.enabled ? ' on' : ''}`}
              onClick={() => set({ enabled: !form.enabled })}
            >
              <span className="am-knob" />
            </button>
            <button className="dv-modal-close" onClick={onClose} title="Close">
              <X size={14} weight="bold" />
            </button>
          </div>
        </div>

        <input
          className="af-name"
          value={form.name}
          onChange={(e) => set({ name: e.target.value })}
          placeholder="Name the rule"
          aria-label="Rule name"
          spellCheck={false}
          autoFocus
        />

        <div className="af-section">When</div>
        <div className="af-when">
          <div className="af-field af-field--kind">
            <span className="af-label">Trigger</span>
            <button type="button" className="af-chip" onClick={openPicker('kind')}>
              {kind.name}
              <CaretDown size={12} />
            </button>
          </div>
          {trigger.kind === 'pipeline' && (
            <div className="af-field af-field--branch">
              <span className="af-label">Branch</span>
              <input
                className="af-input"
                value={trigger.branch}
                onChange={(e) => setTrigger({ branch: e.target.value })}
                placeholder="main"
                aria-label="Branch"
                spellCheck={false}
              />
            </div>
          )}
          {trigger.kind === 'schedule' && (
            <div className="af-field af-field--time">
              <span className="af-label">Time</span>
              <input
                type="time"
                className="af-input"
                value={trigger.time}
                onChange={(e) => setTrigger({ time: e.target.value })}
                aria-label="Time"
              />
            </div>
          )}
          {/*
            project is the one project choice — shown for every trigger
            kind. pipelines etc. watch it (it's in the sentence);
            schedules just open their chat in it
          */}
          <div className="af-field af-field--project">
            <span className="af-label">Project</span>
            <button type="button" className="af-chip" onClick={openPicker('whenProject')}>
              {trigger.project}
              <CaretDown size={12} />
            </button>
          </div>
        </div>
        <p className="af-preview">
          {whenSentence(trigger)}
          {trigger.kind === 'schedule' && ' · This build does not fire on the clock — Run now opens the chat.'}
        </p>

        <div className="af-section">Then</div>
        <div className="af-grid">
          <div className="af-field">
            <span className="af-label">Harness</span>
            <button type="button" className="af-chip" onClick={openPicker('harness')}>
              {harnessName(form.harness)}
              <CaretDown size={12} />
            </button>
          </div>
          <div className="af-field">
            <span className="af-label">Model</span>
            <button type="button" className="af-chip" onClick={openPicker('model')}>
              {model?.name ?? form.model}
              <CaretDown size={12} />
            </button>
          </div>
          <div className="af-field">
            <span className="af-label">Machine</span>
            <button type="button" className="af-chip" onClick={openPicker('device')}>
              {machine?.name ?? 'Choose'}
              <CaretDown size={12} />
            </button>
          </div>
        </div>

        <div className="af-section">Task</div>
        <textarea
          className="af-task"
          value={form.task}
          onChange={(e) => set({ task: e.target.value })}
          placeholder="What the chat should start with"
          aria-label="Task"
          spellCheck={false}
        />

        <div className="af-foot">
          <button type="button" className="dv-act" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="dv-act dv-connect" disabled={!canSave} onClick={save}>
            {rule ? 'Save rule' : 'Create rule'}
          </button>
        </div>

        {picker?.kind === 'kind' && (
          <PickerMenu
            label="Trigger"
            searchPlaceholder="Search triggers"
            items={KINDS}
            groups={null}
            selectedId={trigger.kind}
            onChoose={(id) => {
              setTrigger({ kind: id })
              setPicker(null)
            }}
            onClose={() => setPicker(null)}
            anchor={{ left: picker.x, top: picker.y }}
          />
        )}
        {picker?.kind === 'whenProject' && (
          <PickerMenu
            label="Project"
            searchPlaceholder="Search projects"
            items={PROJECTS}
            groups={null}
            selectedId={trigger.project}
            onChoose={(id) => {
              setTrigger({ project: id })
              setPicker(null)
            }}
            onClose={() => setPicker(null)}
            anchor={{ left: picker.x, top: picker.y }}
          />
        )}
        {picker?.kind === 'harness' && (
          <PickerMenu
            label="Harness"
            searchPlaceholder="Search harnesses"
            items={harnesses}
            groups
            selectedId={form.harness}
            onChoose={(id) => {
              set({ harness: id, model: MODELS[id][0].id })
              setPicker(null)
            }}
            onClose={() => setPicker(null)}
            anchor={{ left: picker.x, top: picker.y }}
          />
        )}
        {picker?.kind === 'model' && (
          <PickerMenu
            label="Model"
            searchPlaceholder="Search models"
            items={modelList}
            groups={null}
            selectedId={model?.id}
            onChoose={(id) => {
              set({ model: id })
              setPicker(null)
            }}
            onClose={() => setPicker(null)}
            anchor={{ left: picker.x, top: picker.y }}
          />
        )}
        {picker?.kind === 'device' && (
          <PickerMenu
            label="Machine"
            searchPlaceholder="Search machines"
            items={world.devices}
            groups={null}
            selectedId={form.deviceId}
            onChoose={(id) => {
              set({ deviceId: id })
              setPicker(null)
            }}
            onClose={() => setPicker(null)}
            anchor={{ left: picker.x, top: picker.y }}
          />
        )}
      </div>
    </div>
  )
}

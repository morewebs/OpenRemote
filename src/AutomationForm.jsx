// The rule editor: name, when (a schedule or a webhook - the kinds with
// real sources), the chat it opens (harness, model, machine), and the
// task it starts with. The project is picked from the registered ones -
// its folder is the chat's own working folder.

import { useEffect, useState } from 'react'
import { X } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { modelDisplayName } from './model-display.js'
import { projectName } from './projects.js'
import './devices.css'
import './automations.css'

const TRIGGERS = [
  { id: 'schedule', name: 'On a schedule' },
  { id: 'webhook', name: 'A webhook' },
]

const HOURS = Array.from({ length: 24 }, (_, h) => String(h).padStart(2, '0'))
const MINUTES = ['00', '15', '30', '45']

// A saved time is "HH:MM". Anything else falls back to 09:00.
function splitTime(value) {
  const match = /^(\d{2}):(\d{2})$/.exec(value ?? '')
  return match ? [match[1], match[2]] : ['09', '00']
}

export default function AutomationForm({ rule, onClose }) {
  const {
    capabilities,
    machines,
    modelsFor,
    projects,
    saveAutomation,
    enableAutomation,
    api,
  } = useConsole()
  // A saved rule carries its id (an edit); a prefilled draft from the
  // starter or the pill carries only fields.
  const editing = rule?.id != null
  const available = (capabilities?.harnesses ?? []).filter((h) => h.available)
  const online = (machines ?? []).filter((m) => m.machine.status === 'online')

  const [name, setName] = useState(rule?.name ?? '')
  const [kind, setKind] = useState(rule?.trigger?.kind ?? 'schedule')
  const [time, setTime] = useState(rule?.trigger?.time ?? '09:00')
  const [harness, setHarness] = useState(rule?.harness ?? available[0]?.id ?? null)
  const [model, setModel] = useState(rule?.model ?? null)
  // A draft or saved rule may name a machine that isn't online - the
  // form preselects one that can actually run the rule.
  const [machineId, setMachineId] = useState(
    online.some((m) => m.machine.id === rule?.machine)
      ? rule.machine
      : online[0]?.machine.id ?? null,
  )
  // The rule's project: picked from the registered ones. A saved rule
  // carries its folder; the picker matches it back to its project and
  // preselects that.
  const [workspace, setWorkspace] = useState(rule?.workspace ?? defaultWorkspace(projects))
  const [task, setTask] = useState(rule?.task ?? '')
  const [enabled, setEnabled] = useState(rule?.enabled ?? true)
  const [picker, setPicker] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  const [models, setModels] = useState([])
  useEffect(() => {
    let cancelled = false
    setModel(null)
    if (!harness) {
      setModels([])
      return
    }
    modelsFor(harness).then((list) => {
      if (!cancelled) setModels(list ?? [])
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [harness])

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape' && !picker) onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose, picker])

  const currentHarness = available.find((h) => h.id === harness)
  const currentMachine = online.find((m) => m.machine.id === machineId)
  // One online machine cannot be chosen - the field still saves its id.
  const oneMachine = online.length <= 1
  // The registered project whose folder the rule's workspace is - the
  // picker's preselection; a rule whose folder no project holds (one
  // saved before projects) keeps its folder as-is.
  const matchedProject =
    (projects ?? []).find((p) => p.folders.includes(workspace)) ?? null
  const noProjects = (projects ?? []).length === 0
  const ready = name.trim() && task.trim() && workspace.trim() && harness && machineId && (kind !== 'schedule' || time.trim())

  const openPicker = (kind_) => (e) => setPicker({ kind: kind_, x: e.clientX, y: e.clientY })

  const save = async () => {
    if (!ready || busy) return
    setBusy(true)
    setError(null)
    try {
      const saved = await saveAutomation({
        ...(editing ? { id: rule.id, enabled } : {}),
        name: name.trim(),
        trigger: {
          kind,
          ...(kind === 'schedule' ? { time: time.trim() } : {}),
          ...(kind === 'webhook' && rule?.trigger?.key ? { key: rule.trigger.key } : {}),
        },
        harness,
        model: model ?? undefined,
        workspace: workspace.trim(),
        machine: machineId,
        task: task.trim(),
      })
      // The form's own switch only rules edits; a create starts enabled.
      if (editing && saved?.enabled !== enabled) await enableAutomation(saved.id, enabled)
      onClose()
    } catch (err) {
      setError(err.message ?? String(err))
      setBusy(false)
    }
  }

  const hookUrl =
    kind === 'webhook' && rule?.trigger?.key && api?.url
      ? `${api.url}/hooks/${rule.id}?key=${rule.trigger.key}`
      : null

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal dv-modal--form af-modal" role="dialog" aria-modal="true" aria-label={editing ? 'Edit automation' : 'New automation'}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">{editing ? 'Edit automation' : 'New automation'}</h2>
          {editing && (
            <button
              className={`am-switch${enabled ? ' on' : ''}`}
              role="switch"
              aria-checked={enabled}
              onClick={() => setEnabled((v) => !v)}
              title={enabled ? 'Enabled' : 'Disabled'}
            />
          )}
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>

        <label className="af-name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Name the rule"
            spellCheck={false}
            aria-label="Rule name"
          />
        </label>

        <div className="af-section">
          <div className="af-label">When</div>
          <div className="af-trigger-row">
            <div className="af-chips">
              {TRIGGERS.map((t) => (
                <button
                  key={t.id}
                  className={`af-chip${kind === t.id ? ' on' : ''}`}
                  aria-pressed={kind === t.id}
                  onClick={() => setKind(t.id)}
                >
                  {t.name}
                </button>
              ))}
            </div>
            {kind === 'schedule' && (
              <span className="af-time">
                <select
                  value={splitTime(time)[0]}
                  aria-label="Hour"
                  onChange={(e) => setTime(`${e.target.value}:${splitTime(time)[1]}`)}
                >
                  {HOURS.map((h) => (
                    <option key={h} value={h}>{h}</option>
                  ))}
                </select>
                <span className="af-time-colon" aria-hidden="true">:</span>
                <select
                  value={MINUTES.includes(splitTime(time)[1]) ? splitTime(time)[1] : '00'}
                  aria-label="Minute"
                  onChange={(e) => setTime(`${splitTime(time)[0]}:${e.target.value}`)}
                >
                  {MINUTES.map((m) => (
                    <option key={m} value={m}>{m}</option>
                  ))}
                </select>
              </span>
            )}
          </div>
          {kind === 'webhook' && (
            <p className="af-when-note">
              {hookUrl
                ? 'Any request to this hook fires the rule - the key in the URL is its credential.'
                : 'The hook URL appears on the rule once it is saved.'}
            </p>
          )}
        </div>

        <div className="af-section">
          <div className="af-label">Chat</div>
          <div className="af-then">
            <div className="af-then-cell">
              <button
                className="af-pick"
                onClick={openPicker('harness')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'harness'}
              >
                {currentHarness?.name ?? 'No harness installed'}
              </button>
              {picker?.kind === 'harness' && (
                <PickerMenu
                  label="Harness"
                  searchPlaceholder="Search harnesses"
                  items={available.map((h) => ({ id: h.id, name: h.name }))}
                  groups={null}
                  selectedId={harness ?? ''}
                  onChoose={(id) => {
                    setHarness(id)
                    setPicker(null)
                  }}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                />
              )}
            </div>
            {models.length > 0 && (
              <div className="af-then-cell">
                <button
                  className="af-pick"
                  onClick={openPicker('model')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'model'}
                >
                  {model ? modelDisplayName(models, model) : `${currentHarness?.name} default`}
                </button>
                {picker?.kind === 'model' && (
                  <PickerMenu
                    label="Model"
                    searchPlaceholder="Search models"
                    items={models.map((m) => ({ id: m.model, name: m.display_name ?? m.model }))}
                    groups={null}
                    selectedId={model ?? ''}
                    onChoose={(id) => {
                      setModel(id)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                  />
                )}
              </div>
            )}
            {!oneMachine && (
              <div className="af-then-cell">
                <button
                  className="af-pick"
                  onClick={openPicker('machine')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'machine'}
                >
                  {currentMachine?.machine.name ?? 'No machine online'}
                </button>
                {picker?.kind === 'machine' && (
                  <PickerMenu
                    label="Machine"
                    searchPlaceholder="Search machines"
                    items={online.map((m) => ({ id: m.machine.id, name: m.machine.name }))}
                    groups={null}
                    selectedId={machineId ?? ''}
                    onChoose={(id) => {
                      setMachineId(id)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                  />
                )}
              </div>
            )}
          </div>
          <div className="af-workspace">
            <span className="af-label">Project</span>
            <span className="af-workspace-row">
              <button
                type="button"
                className="af-pick"
                onClick={openPicker('project')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'project'}
              >
                {matchedProject ? projectName(matchedProject) : 'No project yet'}
              </button>
              {picker?.kind === 'project' && (
                <PickerMenu
                  label="Project"
                  searchPlaceholder="Search projects"
                  wide
                  items={(projects ?? []).map((p) => ({ id: p.folders[0], name: projectName(p) }))}
                  groups={null}
                  selectedId={workspace ?? ''}
                  onChoose={(id) => {
                    setWorkspace(id)
                    setPicker(null)
                  }}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderSubline={(item) => <span className="nc-item-sub">{item.id}</span>}
                />
              )}
            </span>
          </div>
          {noProjects && (
            <p className="af-when-note">
              No projects yet - add one from the home screen's Add project button.
            </p>
          )}
        </div>

        <div className="af-section">
          <div className="af-label">Task</div>
          <textarea
            className="af-task"
            value={task}
            onChange={(e) => setTask(e.target.value)}
            placeholder="What the chat should start with"
            spellCheck={false}
          />
        </div>

        {error && <p className="af-error">{error}</p>}
        <div className="af-foot">
          <button type="button" className="dv-act" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="dv-act dv-connect" disabled={!ready || busy} onClick={save}>
            {busy ? 'Saving…' : editing ? 'Save rule' : 'Create rule'}
          </button>
        </div>
      </div>
    </div>
  )
}

function defaultWorkspace(projects) {
  return projects?.[0]?.folders[0] ?? ''
}

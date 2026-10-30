// The rule editor: name, when (a schedule or a webhook - the kinds with
// real sources), the chat it opens (harness, model, machine), and the
// task it starts with. The project is picked from the registered ones -
// its folder is the chat's own working folder.

import { useEffect, useRef, useState } from 'react'
import { CaretDown, X } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { findModel, modelDisplayName, prettyModelName } from './model-display.js'
import { modelMaker } from './model-maker.js'
import { ModelMark } from './model-marks.jsx'
import { projectName } from './projects.js'
import { formDiffers } from './form-dirty.js'
import { carrySetup, droppedLine, restoreFromCatalog } from './harness-carry.js'
import { ruleMachineState } from './rule-run.js'
import { jobSourceLine } from './chat-to-job.js'
import './devices.css'
import './automations.css'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { pickable } from './harness-manager.js'
import { EndpointManagerModal } from './EndpointManager.jsx'
import { errorCopy } from './error-copy.js'

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
  useBack(true, onClose)
  const panelRef = useRef(null)
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
  // starter, the pill or a chat carries only fields.
  const editing = rule?.id != null
  // A draft read off a chat: its time and project are the two things a chat
  // cannot know, so both start empty and the source line asks for them
  // instead of the form guessing a schedule nobody chose.
  const fromChat = rule?.fromChat === true
  const available = (capabilities?.harnesses ?? []).filter(pickable)
  const online = (machines ?? []).filter((m) => m.machine.status === 'online')
  // Where the rule's own machine stands: no machine (this computer), in the
  // account and online, in the account but asleep, or gone from it. An edit
  // keeps the saved machine: moving a rule to another computer is a choice the
  // person makes in the picker, never something a save does by itself.
  const savedMachine = rule?.machine ? ruleMachineState(rule, machines) : null

  const [name, setName] = useState(rule?.name ?? '')
  const [kind, setKind] = useState(rule?.trigger?.kind ?? 'schedule')
  const [time, setTime] = useState(rule?.trigger?.time ?? (fromChat ? '' : '09:00'))
  const [harness, setHarness] = useState(rule?.harness ?? available[0]?.id ?? null)
  const [model, setModel] = useState(rule?.model ?? null)
  // The rule's permission word - the harness's own vocabulary, carried to
  // the chat the rule fires. Unset means the harness's default holds.
  const [permission, setPermission] = useState(rule?.permission_mode ?? null)
  // A saved rule keeps the machine it names, online or not: preselecting the
  // first online one here used to move the rule as a side effect of any edit.
  // A new rule or a draft still starts on the first machine that can run it.
  const [machineId, setMachineId] = useState(rule?.machine ?? online[0]?.machine.id ?? null)
  // The saved machine has left the account and nothing has replaced it: saving
  // would write a dead id, so the form asks for a machine first (picking one is
  // the person's own move, which is why the default is not replaced for them).
  const machineGone = Boolean(savedMachine) && savedMachine.state === 'gone' && machineId === rule.machine
  // The rule's project: picked from the registered ones. A saved rule
  // carries its folder; the picker matches it back to its project and
  // preselects that. A chat with no folder starts here empty - the first
  // registered project would be a workspace the chat never ran in.
  const [workspace, setWorkspace] = useState(
    rule?.workspace ?? (fromChat ? '' : defaultWorkspace(projects)),
  )
  const [task, setTask] = useState(rule?.task ?? '')
  const [enabled, setEnabled] = useState(rule?.enabled ?? true)
  const [picker, setPicker] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [managingEndpoints, setManagingEndpoints] = useState(false)

  const [models, setModels] = useState([])
  // An unreachable daemon must not read as "this harness has no models".
  const [modelsUnavailable, setModelsUnavailable] = useState(false)
  // What the switch could not carry, said once in the form's own line.
  const [carryNote, setCarryNote] = useState(null)
  // The harness the form opened with. A rule's model and permission word
  // belong to the rule, so the first run of this effect keeps them: only a
  // switch the person makes asks harness-carry.js what can come along.
  const harnessAtOpen = useRef(harness)
  // A model the switch dropped, held until the new catalog answers: a person
  // who picked Opus should not lose it because the list was still in flight.
  const pendingCarry = useRef(null)
  const droppedAtSwitch = useRef([])
  const carryHarnessName = useRef(null)
  useEffect(() => {
    let cancelled = false
    setModelsUnavailable(false)
    if (harness !== harnessAtOpen.current) {
      const next = available.find((h) => h.id === harness) ?? null
      const { kept, dropped } = carrySetup(
        { model, permission },
        { models: null, permissionModes: next?.permission_modes ?? [] },
      )
      setModel(kept.model)
      setPermission(kept.permission)
      pendingCarry.current = dropped.includes('model') ? model : null
      droppedAtSwitch.current = dropped
      carryHarnessName.current = next?.name ?? harness
      setCarryNote(droppedLine(dropped, carryHarnessName.current))
      harnessAtOpen.current = harness
    }
    if (!harness) {
      setModels([])
      return
    }
    modelsFor(harness)
      .then((list) => {
        if (cancelled) return
        setModels(list ?? [])
        // The catalog arrived: a model the switch had to drop comes back if
        // this harness carries the same row, and the note stops naming it.
        const back = restoreFromCatalog(pendingCarry.current, list ?? [])
        if (back) {
          pendingCarry.current = null
          setModel(back)
          droppedAtSwitch.current = droppedAtSwitch.current.filter((key) => key !== 'model')
          setCarryNote(droppedLine(droppedAtSwitch.current, carryHarnessName.current))
        }
      })
      .catch(() => {
        // An unreachable daemon must not look like "this harness has no
        // models": the list stays empty and the picker says it did not load.
        if (!cancelled) {
          setModels([])
          setModelsUnavailable(true)
        }
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [harness])

  // The fields as they were when the form opened, taken once on the first
  // render. `dirty` is then a comparison, not a flag each setter has to
  // remember to raise: a field added later is covered by adding it here.
  const opened = useRef(null)
  if (opened.current === null) {
    opened.current = { name, kind, time, harness, model, permission, machineId, workspace, task, enabled }
  }
  const dirty = formDiffers(
    { name, kind, time, harness, model, permission, machineId, workspace, task, enabled },
    opened.current,
  )
  const [closeHint, setCloseHint] = useState(false)

  // Closing the form by the way out is still a cancel; closing it by the
  // backdrop or by Esc is the accident this guard exists for. A half-written
  // rule is not something a stray click should be able to delete, so the two
  // silent ways out stand down while there is anything to lose, and say which
  // button does discard on purpose.
  const requestClose = () => {
    if (dirty) {
      setCloseHint(true)
      return
    }
    onClose()
  }

  // Esc closes the form, unless a picker is open over it - that layer answers
  // its own Esc - or a dialog is: "Manage endpoints" holds a layer of its own,
  // so the half-written rule is never closed by the key that closed it. Every
  // dialog shares the plumbing in dialog-focus.js: focus in, Tab kept inside,
  // the app inert behind, focus back on the opener.
  useDialog(panelRef, requestClose, { esc: !picker })

  const currentHarness = available.find((h) => h.id === harness)
  const currentMachine = online.find((m) => m.machine.id === machineId)
  // What the picker shows for the current choice: the online machine's own
  // name, or the saved machine's words when it is asleep or gone (never an id),
  // or nothing when there is no machine at all.
  const machineName =
    currentMachine?.machine.name ??
    (machineId && machineId === rule?.machine ? savedMachine?.label : null) ??
    null
  // The machine this rule already targets, kept in the picker even when it is
  // not online: it is where the rule runs, and seeing it marked is how a person
  // knows a save is not moving the rule.
  const machineOptions = [
    ...online.map((m) => ({ id: m.machine.id, name: m.machine.name, veiled: true })),
    ...(savedMachine && savedMachine.state !== 'online' && rule?.machine
      ? [
          {
            id: rule.machine,
            name:
              savedMachine.state === 'gone'
                ? savedMachine.label
                : `${savedMachine.label} · not online`,
            veiled: savedMachine.state !== 'gone',
          },
        ]
      : []),
  ]
  // The two halves of the rule's time, as its pickers show them: a saved
  // minute the list does not carry reads as 00, which is what picking from
  // that list would have written.
  const [hour, savedMinute] = splitTime(time)
  const minute = MINUTES.includes(savedMinute) ? savedMinute : '00'
  // One online machine cannot be chosen - the field still saves its id.
  const oneMachine = online.length <= 1
  // The registered project whose folder the rule's workspace is - the
  // picker's preselection; a rule whose folder no project holds (one
  // saved before projects) keeps its folder as-is.
  const matchedProject =
    (projects ?? []).find((p) => p.folders.includes(workspace)) ?? null
  const noProjects = (projects ?? []).length === 0
  const ready =
    name.trim() &&
    task.trim() &&
    workspace.trim() &&
    harness &&
    machineId &&
    !machineGone &&
    (kind !== 'schedule' || time.trim())

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
        permission_mode: permission ?? undefined,
        workspace: workspace.trim(),
        machine: machineId,
        task: task.trim(),
      })
      // The form's own switch only rules edits; a create starts enabled.
      if (editing && saved?.enabled !== enabled) await enableAutomation(saved.id, enabled)
      onClose()
    } catch (err) {
      setError(errorCopy(err))
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
        if (e.target === e.currentTarget) requestClose()
      }}
    >
      <div className="dv-modal dv-modal--form af-modal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={editing ? 'Edit automation' : 'New automation'}>
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
            // The dialog would otherwise hand the keyboard to its own close
            // button, and the first word a person types would go nowhere.
            autoFocus
          />
        </label>

        {/* Where the values came from, and what is still the person's to
            choose. It rewrites itself as they work, so it never asks for
            something they have already picked. */}
        {fromChat && (
          <p className="af-when-note">
            {jobSourceLine({
              shortened: rule?.shortened === true,
              needsProject: !workspace.trim() && (projects ?? []).length > 0,
              when: kind,
              timed: Boolean(time.trim()),
            })}
          </p>
        )}

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
                {/* Two pickers, one kind each: an hour open on the clock
                    never answers for the minute beside it. A draft read off
                    a chat has no time yet, and says so instead of showing a
                    schedule nobody chose; picking the hour starts it. */}
                {time ? (
                  <>
                    <button
                      type="button"
                      aria-label="Hour"
                      aria-haspopup="listbox"
                      aria-expanded={picker?.kind === 'hour'}
                      onClick={openPicker('hour')}
                    >
                      {hour}
                      <CaretDown size={10} />
                    </button>
                    <span className="af-time-colon" aria-hidden="true">:</span>
                    <button
                      type="button"
                      aria-label="Minute"
                      aria-haspopup="listbox"
                      aria-expanded={picker?.kind === 'minute'}
                      onClick={openPicker('minute')}
                    >
                      {minute}
                      <CaretDown size={10} />
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="af-time-empty"
                    aria-label="Time"
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'hour'}
                    onClick={openPicker('hour')}
                  >
                    Pick a time
                    <CaretDown size={10} />
                  </button>
                )}
                {picker?.kind === 'hour' && (
                  <PickerMenu
                    label="Hour"
                    searchPlaceholder="Search hours"
                    items={HOURS.map((h) => ({ id: h, name: h }))}
                    groups={null}
                    selectedId={hour}
                    onChoose={(id) => {
                      setTime(`${id}:${minute}`)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                  />
                )}
                {picker?.kind === 'minute' && (
                  <PickerMenu
                    label="Minute"
                    searchPlaceholder="Search minutes"
                    items={MINUTES.map((m) => ({ id: m, name: m }))}
                    groups={null}
                    selectedId={minute}
                    onChoose={(id) => {
                      setTime(`${hour}:${id}`)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                  />
                )}
              </span>
            )}
          </div>
          {kind === 'webhook' && (
            <p className="af-when-note">
              {hookUrl
                ? 'The hook answers only on this computer, at the address shown on the saved '
                  + 'rule, and the key in that URL is its credential. That address changes each '
                  + 'time OpenRemote starts, so a script reads the current one first rather than '
                  + 'hardcoding it.'
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
                data-unset={!currentHarness || undefined}
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
            {(currentHarness?.permission_modes ?? []).length > 0 && (
              <div className="af-then-cell">
                <button
                  className="af-pick"
                  data-unset={!permission || undefined}
                  onClick={openPicker('permission')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'permission'}
                  title="What the harness may do without asking when the rule fires"
                >
                  {permission ?? 'Permission'}
                </button>
                {picker?.kind === 'permission' && (
                  <PickerMenu
                    label="Permission"
                    searchPlaceholder="Search permission words"
                    items={(currentHarness?.permission_modes ?? []).map((word) => ({
                      id: word.w ?? word,
                      name: word.l ?? word.w ?? word,
                    }))}
                    groups={null}
                    selectedId={permission ?? ''}
                    onChoose={(id) => {
                      setPermission(id)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                    renderSubline={(item) =>
                      item.id === currentHarness?.default_permission ? (
                        <span className="nc-item-sub">
                          {currentHarness?.name} runs this by default
                        </span>
                      ) : null
                    }
                  />
                )}
              </div>
            )}
            {models.length > 0 && (
              <div className="af-then-cell">
                <button
                  className="af-pick"
                  onClick={openPicker('model')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'model'}
                >
                  {(() => {
                    // The shown model's own maker mark - the chosen
                    // one, else the harness's default row.
                    const row =
                      (model && findModel(models, model)) ?? models.find((m) => m.is_default) ?? null
                    const maker = modelMaker(harness, row?.model, row?.resolved_model)
                    return maker ? <ModelMark maker={maker} size={14} /> : null
                  })()}
                  {model ? modelDisplayName(models, model) : `${currentHarness?.name} default`}
                </button>
                {picker?.kind === 'model' && (
                  <PickerMenu
                    label="Model"
                    searchPlaceholder="Search models"
                    items={models.map((m) => ({
                      id: m.model,
                      name: prettyModelName(m.display_name ?? m.model),
                      resolved_model: m.resolved_model,
                    }))}
                    groups={null}
                    selectedId={model ?? ''}
                    onChoose={(id) => {
                      setModel(id)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                    renderIcon={(m) => (
                      <ModelMark maker={modelMaker(harness, m.id, m.resolved_model)} size={14} />
                    )}
                    footer={
                      currentHarness?.custom_endpoint_supported ? (
                        <button
                          type="button"
                          onClick={() => {
                            setPicker(null)
                            // open the endpoint manager for this harness on the
                            // same machine the rule will run on
                            setManagingEndpoints(true)
                          }}
                        >
                          Manage models…
                        </button>
                      ) : null
                    }
                  />
                )}
              </div>
            )}
            {/* The picker is hidden when there is nothing to choose between,
                except when the rule's own machine has left the account: then it
                is the only way to answer the refusal, so it has to be there. */}
            {(!oneMachine || machineGone) && (
              <div className="af-then-cell">
                <button
                  className="af-pick"
                  data-unset={!currentMachine || undefined}
                  onClick={openPicker('machine')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'machine'}
                >
                  {machineName ? <span className="pv">{machineName}</span> : 'No machine online'}
                </button>
                {picker?.kind === 'machine' && (
                  <PickerMenu
                    label="Machine"
                    searchPlaceholder="Search machines"
                    items={machineOptions}
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
          {(carryNote || modelsUnavailable) && (
            <p className="af-when-note" role="status">
              {carryNote ?? 'The model list did not load: the harness may still be starting.'}
            </p>
          )}
          {machineGone && (
            // The rule's machine left the account: the save keeps it by default,
            // so it is refused until the person picks where the rule should run.
            // Moving a rule is never something a save does on its own.
            <p className="af-when-note" role="status">
              The machine this rule ran on is no longer in your account. Pick where it should run
              before saving.
            </p>
          )}
          <div className="af-workspace">
            <span className="af-label">Project</span>
            <span className="af-workspace-row">
              <button
                type="button"
                className="af-pick"
                data-unset={noProjects || undefined}
                onClick={openPicker('project')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'project'}
              >
                {matchedProject ? <span className="pv">{projectName(matchedProject)}</span> : 'No project yet'}
              </button>
              {picker?.kind === 'project' && (
                <PickerMenu
                  label="Project"
                  searchPlaceholder="Search projects"
                  wide
                  items={(projects ?? []).map((p) => ({ id: p.folders[0], name: projectName(p), veiled: true }))}
                  groups={null}
                  selectedId={workspace ?? ''}
                  onChoose={(id) => {
                    setWorkspace(id)
                    setPicker(null)
                  }}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderSubline={(item) => <span className="nc-item-sub pv">{item.id}</span>}
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
        {closeHint && dirty && (
          <p className="af-when-note">
            This rule has unsaved changes. Cancel discards it.
          </p>
        )}
        {managingEndpoints && (
          <EndpointManagerModal
            harness={currentHarness.id}
            deviceId={currentMachine?.machine.id ?? null}
            where={currentMachine?.machine.name ?? 'this computer'}
            onClose={() => setManagingEndpoints(false)}
          />
        )}
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

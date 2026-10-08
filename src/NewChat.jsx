import { useEffect, useState } from 'react'
import { ArrowUp, House, FolderOpen, Lightning, Plus } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import FolderPickerModal from './FolderPickerModal.jsx'
import { machinePickerItems } from './cloud.js'
import { HarnessMark } from './brand-marks.jsx'
import SignInModal from './SignInModal.jsx'
import HarnessManagerModal from './HarnessManagerModal.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { findModel, modelDisplayName } from './model-display.js'
import { SIGNIN_HARNESSES } from './harness-manager.js'
import { loadDefaults } from './defaults.js'
import { projectName } from './projects.js'
import './newchat.css'
import './composer.css'

// The harnesses whose own login command the daemon can relay - the
// sign-in prompt offers the relay only there; the others say their own
// honest words.

const PLATFORM_LABEL = { windows: 'Windows', macos: 'macOS', linux: 'Linux' }

export default function NewChat({ onOpen, mode = 'local', addProjectOpen = false, onAddProjectDone }) {
  const {
    connection, capabilities, sessions, chats, createChat, modelsFor, devices, thisDevice,
    capabilitiesFor, apiFor, projects, addProject,
  } = useConsole()
  // In Cloud a chat can run on this computer or one of the account's
  // machines; in Local it always runs here.
  const [target, setTarget] = useState(null)
  const [targetCaps, setTargetCaps] = useState(null)
  const [picking, setPicking] = useState(false)
  const [addProjectError, setAddProjectError] = useState(null)
  const cloudMode = mode === 'cloud'
  const machines = cloudMode ? machinePickerItems(devices, thisDevice) : []
  const targetItem = machines.find((m) => m.id === target) ?? null
  const targetOffline = Boolean(target && targetItem && !targetItem.online)
  const [defaults] = useState(() => loadDefaults())
  const [text, setText] = useState('')
  // The project the chat runs in: one of the registered projects, or
  // null - a chat that belongs to no project and runs in the home folder.
  const [projectId, setProjectId] = useState(null)
  const [picker, setPicker] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [harness, setHarness] = useState(() => loadDefaults().harness)
  const [model, setModel] = useState(null)
  const [models, setModels] = useState([])
  const [modelsPending, setModelsPending] = useState(true)
  const [fast, setFast] = useState(false)
  const [signInFor, setSignInFor] = useState(null)
  const [managingHarnesses, setManagingHarnesses] = useState(false)

  // The sidebar's + opens the same Add project flow the home screen's
  // button opens - one way in, whichever surface asked.
  useEffect(() => {
    if (addProjectOpen) setPicking(true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [addProjectOpen])

  // The registered projects on the machine the chat will run on. A
  // project belongs to the machine that owns its folder, so the list
  // follows the target.
  const [targetProjects, setTargetProjects] = useState([])
  useEffect(() => {
    let cancelled = false
    if (!connection.state === 'connected') return
    apiFor(target)
      .projects()
      .then((list) => {
        if (!cancelled) setTargetProjects(list ?? [])
      })
      .catch(() => {
        if (!cancelled) setTargetProjects([])
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, connection.state, projects])

  // A machine's own harnesses, fetched when a chat aims at it.
  useEffect(() => {
    let cancelled = false
    setTargetCaps(null)
    if (target) {
      capabilitiesFor(target).then((caps) => {
        if (!cancelled) setTargetCaps(caps)
      })
    }
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target])

  // Only what the machine reports installed - no dead UI.
  const available = ((target ? targetCaps : capabilities)?.harnesses ?? []).filter((h) => h.available)
  const currentHarness = available.find((h) => h.id === harness) ?? available[0] ?? null
  // The picked harness's own sign-in fact: a harness that isn't signed in
  // says so right here - the task is about to run on it.
  const needsSignIn = currentHarness && currentHarness.signed_in === false

  // The model slot: filled where the harness advertises, reserved
  // (not rendered) where it doesn't. Fast mode follows the same rule -
  // and both choices are per-harness, so switching resets them. The
  // saved defaults preselect their own harness's slots.
  useEffect(() => {
    let cancelled = false
    const defaults = loadDefaults()
    setModel(currentHarness?.id === defaults.harness ? defaults.model : null)
    setFast(false)
    if (!currentHarness) {
      setModels([])
      setModelsPending(false)
      return
    }
    setModelsPending(true)
    modelsFor(currentHarness.id, target).then((list) => {
      if (!cancelled) {
        setModels(list ?? [])
        setModelsPending(false)
      }
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentHarness?.id, connection.state, target])

  const chosenProject = targetProjects.find((p) => p.id === projectId) ?? null
  // The chat's workspace is the project's folder; no project, no
  // workspace - the chat runs in the home folder on its machine.
  const workspace = chosenProject?.folders[0] ?? null

  const canSend =
    text.trim().length > 0 && !busy && currentHarness && connection.state === 'connected'
    && !needsSignIn && !targetOffline
  // The harness's own pre-send facts, each in its own words: what its
  // config says a fresh chat runs (grok's config.toml, agy's settings,
  // claude's priority chain), then its catalog's default marker (codex's
  // isDefault, claude's `default` alias - the row its own picker marks
  // recommended). Neither, and the slot stays reserved.
  const configDefault = currentHarness?.default_model ?? null
  const catalogDefault = models.find((m) => m.is_default)?.model ?? null
  const resolvedDefault = configDefault ?? catalogDefault
  // The pre-send fact's label, in the harness's own words: the config's
  // model name where the catalog knows it, the id where it doesn't.
  const resolvedDefaultName = resolvedDefault ? modelDisplayName(models, resolvedDefault) : null
  const defaultLabel =
    resolvedDefaultName && currentHarness?.default_effort
      ? `${resolvedDefaultName} · ${currentHarness.default_effort}`
      : resolvedDefaultName
  const inProgress = (sessions ?? []).filter((s) => ['starting', 'working', 'waiting'].includes(s.status))
  // The effort slot follows the model slot's rule: only where the active
  // model advertises tiers. A saved default that the active model doesn't
  // offer is dropped, not rendered - never a dead selection.
  const activeModel = model ? models.find((m) => m.model === model) : null
  const effortTiers = activeModel?.reasoning_efforts ?? []
  const [effort, setEffort] = useState(null)
  const savedEffort = effort ?? defaults.efforts?.[currentHarness?.id] ?? null
  const chosenEffort = effortTiers.includes(savedEffort) ? savedEffort : null
  // The model the chat runs from its first breath: the user's pick, else
  // the catalog's default where the harness's config names none. The
  // daemon resolves the config case itself at create (pairing grok's
  // default effort with it); a catalog-only default - claude's `default`
  // alias - would otherwise stay unnamed until the first turn's init
  // frame reports it.
  const startModel = model ?? (configDefault ? null : catalogDefault)

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const start = async () => {
    if (!canSend) return
    setBusy(true)
    setError(null)
    try {
      const id = await createChat(
        text.trim(),
        workspace,
        currentHarness.id,
        startModel ?? undefined,
        fast || undefined,
        chosenEffort ?? undefined,
        { deviceId: target, synced: cloudMode },
      )
      setText('')
      onOpen(id)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  // The one way a folder becomes a project: pick it on the machine that
  // will own it, and it registers there. The same dialog serves both
  // surfaces - a chat's project pick and the Add project button.
  const chooseProjectFolder = async (folder) => {
    setPicking(false)
    setAddProjectError(null)
    try {
      const project = await addProject([folder], { deviceId: target })
      setProjectId(project.id)
    } catch (err) {
      // The daemon's own words - a folder that is already a project, a
      // path that is not a folder - surface right beside the flow.
      setAddProjectError(err.message ?? String(err))
    }
  }

  // Switching machines: a project belongs to one machine, so the choice
  // starts over.
  const chooseTarget = (id) => {
    setTarget(id)
    setProjectId(null)
    setPicker(null)
  }

  return (
    <div className="newchat">
      <h1 className="nc-greeting">What are we working on?</h1>
      {needsSignIn && (
        <div className="nc-signin">
          <div className="nc-signin-note">
            {SIGNIN_HARNESSES.has(currentHarness.id) ? (
              <>
                {harnessName(currentHarness.id)} isn’t signed in on this computer. Sign in to
                start tasks with it.
              </>
            ) : (
              <>
                {harnessName(currentHarness.id)} isn’t signed in - it signs in through its own
                setup.
              </>
            )}
          </div>
          {SIGNIN_HARNESSES.has(currentHarness.id) && (
            <button className="nc-signin-btn" onClick={() => setSignInFor(currentHarness.id)}>
              Sign in
            </button>
          )}
        </div>
      )}
      <div className="nc-box">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              start()
            }
          }}
          placeholder="Describe the task"
          autoFocus
          spellCheck={false}
        />
        <div className="nc-foot">
          <div className="nc-pickers">
            {modelsPending ? (
              <span className="nc-skeleton" aria-hidden="true" />
            ) : (models.length > 0 || resolvedDefault) && (
              <>
                {models.length > 0 ? (
                  <button
                    className="nc-meta"
                    onClick={openPicker('model')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'model'}
                    title="Model"
                  >
                    {model ? modelDisplayName(models, model) : defaultLabel}
                  </button>
                ) : (
                  // No catalog to pick from, but the harness's own config
                  // says what a fresh chat runs - its words, shown as the
                  // fact it is. The wire's first-turn truth replaces it.
                  <span className="nc-meta nc-static" title="The model this harness runs, from its own config">
                    {defaultLabel}
                  </span>
                )}
                {picker?.kind === 'model' && (
                  <PickerMenu
                    label="Model"
                    searchPlaceholder="Search models"
                    wide
                    items={models.map((m) => ({
                      id: m.model,
                      name: m.display_name ?? m.model,
                      reasoning_efforts: m.reasoning_efforts,
                    }))}
                    groups={null}
                    selectedId={model ?? ''}
                    onChoose={(id) => {
                      setModel(id)
                      setEffort(null)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                    renderSubline={(m) =>
                      m.reasoning_efforts?.length ? (
                        <span className="nc-item-sub">
                          {m.reasoning_efforts.join(' · ')}
                        </span>
                      ) : null
                    }
                  />
                )}
                {effortTiers.length > 0 && (
                  <button
                    type="button"
                    className="nc-meta"
                    onClick={openPicker('effort')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'effort'}
                    title="The harness's own effort tier"
                  >
                    {chosenEffort ?? 'Effort'}
                  </button>
                )}
                {picker?.kind === 'effort' && (
                  <PickerMenu
                    label="Effort"
                    searchPlaceholder="Search efforts"
                    items={effortTiers.map((t) => ({ id: t, name: t }))}
                    groups={null}
                    selectedId={chosenEffort ?? ''}
                    onChoose={(id) => {
                      setEffort(id)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                  />
                )}
                <span className="nc-via">via</span>
              </>
            )}
            <button
              className="nc-meta"
              onClick={openPicker('harness')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'harness'}
              title="Harness"
            >
              <HarnessMark harness={currentHarness?.id} size={16} />
              {currentHarness?.name ?? 'No harness installed'}
            </button>
            {picker?.kind === 'harness' && (
              <PickerMenu
                label="Harness"
                searchPlaceholder="Search harnesses"
                items={available.map((h) => ({ id: h.id, name: h.name }))}
                renderIcon={(h) => <HarnessMark harness={h.id} size={16} />}
                groups={null}
                selectedId={currentHarness?.id ?? ''}
                onChoose={(id) => {
                  setHarness(id)
                  // Effort words belong to the harness's own catalog - a
                  // switch resets the choice to the new harness's default.
                  setEffort(null)
                  setPicker(null)
                }}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                footer={
                  // The way in to the harness manager - the only row in
                  // the empty state, and the honest answer when the list
                  // is missing something.
                  <button
                    type="button"
                    onClick={() => {
                      setPicker(null)
                      setManagingHarnesses(true)
                    }}
                  >
                    Manage harnesses…
                  </button>
                }
              />
            )}
            {currentHarness?.fast_supported && (
              <button
                className={`nc-meta nc-fast${fast ? ' on' : ''}`}
                onClick={() => setFast((v) => !v)}
                aria-pressed={fast}
                title="The harness's own fast mode - its speed tier, never a model switch"
              >
                <Lightning size={13} weight={fast ? 'fill' : 'light'} />
                Fast
              </button>
            )}
          </div>
          <div className="nc-send-group">
            <button className="nc-send" onClick={start} disabled={!canSend} title="Start task">
              <ArrowUp size={15} weight="bold" />
            </button>
          </div>
        </div>
      </div>
      <div className="nc-under">
        <div className="nc-below">
          {/* The project is picked, never typed: the registered projects
              on the machine the chat runs on, "No project" for a chat
              that belongs to none, and the one way to add - the picker's
              own Add project row opens the folder picker. */}
          <div className="nc-device">
            <button
              className="nc-meta"
              onClick={openPicker('project')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'project'}
              title={chosenProject ? workspace : 'A chat can run in a project, or in no folder at all'}
            >
              <FolderOpen size={13} weight="light" />
              {chosenProject ? projectName(chosenProject) : 'No project'}
            </button>
            {picker?.kind === 'project' && (
              <PickerMenu
                label="Project"
                searchPlaceholder="Search projects"
                wide
                items={[
                  { id: '__none__', name: 'No project' },
                  ...targetProjects.map((p) => ({ id: p.id, name: projectName(p) })),
                ]}
                groups={null}
                selectedId={chosenProject?.id ?? '__none__'}
                onChoose={(id) => {
                  setProjectId(id === '__none__' ? null : id)
                  setPicker(null)
                }}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                renderSubline={(item) =>
                  item.id === '__none__' ? null : (
                    <span className="nc-item-sub">
                      {targetProjects.find((p) => p.id === item.id)?.folders[0]}
                    </span>
                  )
                }
                footer={
                  <button
                    type="button"
                    onClick={() => {
                      setPicker(null)
                      setPicking(true)
                    }}
                  >
                    <Plus size={12} weight="bold" /> Add project…
                  </button>
                }
              />
            )}
          </div>
          {cloudMode ? (
            <div className="nc-device">
              <button
                className="nc-meta"
                onClick={openPicker('machine')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'machine'}
                title="Where the chat runs"
              >
                <span className={`nc-dev-dot nc-dev-dot--${targetOffline ? 'offline' : 'online'}`} />
                {targetItem?.name ?? 'This computer'}
              </button>
              {picker?.kind === 'machine' && (
                <PickerMenu
                  label="Run on"
                  searchPlaceholder="Search machines"
                  items={machines.map((m) => ({ ...m, id: m.id ?? 'here' }))}
                  groups={null}
                  selectedId={target ?? 'here'}
                  onChoose={(id) => chooseTarget(id === 'here' ? null : id)}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderIcon={(m) =>
                    m.here ? (
                      <House size={13} weight="light" />
                    ) : (
                      <span className={`nc-dev-dot nc-dev-dot--${m.online ? 'online' : 'offline'}`} />
                    )
                  }
                  renderSubline={(m) =>
                    m.here ? null : (
                      <span className="nc-item-sub">{m.online ? PLATFORM_LABEL[m.platform] ?? m.platform : 'Offline'}</span>
                    )
                  }
                />
              )}
            </div>
          ) : (
            <span className="nc-meta nc-static">
              <House size={13} weight="light" />
              This computer
            </span>
          )}
        </div>
        <button type="button" className="nc-browse" onClick={() => setPicking(true)} title="Register a folder as a project">
          <Plus size={13} weight="bold" />
          Add project
        </button>
        {addProjectError && <p className="nc-error">{addProjectError}</p>}
        {targetOffline && <p className="nc-error">{targetItem.name} is offline.</p>}
        {picking && (
          <FolderPickerModal
            api={apiFor(target)}
            where={targetItem?.name ?? 'this computer'}
            start={workspace}
            onChoose={chooseProjectFolder}
            onClose={() => {
              setPicking(false)
              onAddProjectDone?.()
            }}
          />
        )}
        {connection.state === 'error' && <p className="nc-error">{connection.error}</p>}
        {error && <p className="nc-error">{error}</p>}
        {signInFor && (
          <SignInModal
            harnessId={signInFor}
            onDone={() => setSignInFor(null)}
          />
        )}
        {managingHarnesses && (
          <HarnessManagerModal
            onClose={() => setManagingHarnesses(false)}
            onSignIn={(id) => {
              setManagingHarnesses(false)
              setSignInFor(id)
            }}
          />
        )}
        {inProgress.length > 0 && (
          <div className="nc-resume">
            <div className="nc-resume-label">In progress</div>
            {inProgress.map((session) => {
              const chat = chats[session.id]
              const title = chat?.title ?? 'A new task'
              return (
                <button key={session.id} type="button" onClick={() => onOpen(session.id)}>
                  <span className={`dot dot--${chat?.status ?? session.status}`} />
                  <span className="nc-resume-title">{title}</span>
                  <span className="nc-resume-meta">
                    {session.status === 'waiting'
                      ? 'Needs a decision'
                      : harnessName(session.harness)}
                  </span>
                </button>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}

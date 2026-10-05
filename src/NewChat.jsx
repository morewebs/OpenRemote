import { useEffect, useState } from 'react'
import { ArrowUp, House, FolderOpen, Lightning } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { HarnessMark } from './brand-marks.jsx'
import SignInModal from './SignInModal.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { loadDefaults } from './defaults.js'
import { loadRecentWorkspaces, pushRecentWorkspace } from './settings.js'
import { canPickFolder, pickFolder } from './pick-folder.js'
import './newchat.css'
import './composer.css'

// The harnesses whose own login command the daemon can relay - the
// sign-in prompt offers the relay only there; the others say their own
// honest words.
const SIGNIN_HARNESSES = new Set(['claude', 'codex', 'grok'])

export default function NewChat({ onOpen }) {
  const { connection, capabilities, sessions, chats, createChat, modelsFor } = useConsole()
  const [defaults] = useState(() => loadDefaults())
  const [text, setText] = useState('')
  const [workspace, setWorkspace] = useState(() => defaults.workspace ?? loadRecentWorkspaces()[0] ?? null)
  const [picker, setPicker] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [harness, setHarness] = useState(() => loadDefaults().harness)
  const [model, setModel] = useState(null)
  const [models, setModels] = useState([])
  const [modelsPending, setModelsPending] = useState(true)
  const [fast, setFast] = useState(false)
  const [signInFor, setSignInFor] = useState(null)

  // Only what the daemon reports installed - no dead UI.
  const available = (capabilities?.harnesses ?? []).filter((h) => h.available)
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
    modelsFor(currentHarness.id).then((list) => {
      if (!cancelled) {
        setModels(list ?? [])
        setModelsPending(false)
      }
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentHarness?.id, connection.state])

  const canSend =
    text.trim().length > 0 && workspace && !busy && currentHarness && connection.state === 'connected'
    && !needsSignIn
  // The harness's own pre-send fact: what its config says a fresh chat
  // runs, in its own words (grok's config.toml, agy's settings, claude's
  // priority chain) - none where the harness says nothing (the slot stays
  // reserved).
  const resolvedDefault =
    // The pre-send fact: the harness's own config first, then its own
    // default-catalog marker (codex's isDefault - the model a fresh
    // thread runs, the same fact thread/start echoes at create).
    currentHarness?.default_model ?? models.find((m) => m.is_default)?.model ?? null
  const defaultLabel =
    resolvedDefault && currentHarness?.default_effort
      ? `${resolvedDefault} · ${currentHarness.default_effort}`
      : resolvedDefault
  const recents = loadRecentWorkspaces()
  const active = workspace ?? ''
  const inProgress = (sessions ?? []).filter((s) => ['starting', 'working', 'waiting'].includes(s.status))
  // The effort slot follows the model slot's rule: only where the active
  // model advertises tiers. A saved default that the active model doesn't
  // offer is dropped, not rendered - never a dead selection.
  const activeModel = model ? models.find((m) => m.model === model) : null
  const effortTiers = activeModel?.reasoning_efforts ?? []
  const [effort, setEffort] = useState(null)
  const savedEffort = effort ?? defaults.efforts?.[currentHarness?.id] ?? null
  const chosenEffort = effortTiers.includes(savedEffort) ? savedEffort : null

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const start = async () => {
    if (!canSend) return
    setBusy(true)
    setError(null)
    try {
      const id = await createChat(text.trim(), workspace, currentHarness.id, model ?? undefined, fast || undefined, chosenEffort ?? undefined)
      pushRecentWorkspace(workspace)
      setText('')
      onOpen(id)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  // The native folder dialog - one click instead of a typed absolute path.
  const browse = async () => {
    const path = await pickFolder()
    if (!path) return
    setWorkspace(path)
    pushRecentWorkspace(path)
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
                    {model ?? defaultLabel}
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
          {/* The workspace is picked, never typed: the recents picker
              when there are any, the OS folder dialog beside it. An
              empty recents list is no dead control - Browse is the
              affordance. */}
          {recents.length > 0 && (
            <div className="nc-device">
              <button
                className="nc-meta"
                onClick={openPicker('workspace')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'workspace'}
                title={active || 'The folder the harness works in'}
              >
                <FolderOpen size={13} weight="light" />
                {active ? active.split(/[\\/]/).filter(Boolean).pop() : 'Choose a folder'}
              </button>
              {picker?.kind === 'workspace' && (
                <PickerMenu
                  label="Workspace"
                  searchPlaceholder="Search workspaces"
                  wide
                  items={recents.map((p) => ({
                    id: p,
                    name: p.split(/[\\/]/).filter(Boolean).pop(),
                  }))}
                  groups={null}
                  selectedId={active}
                  onChoose={(id) => {
                    setWorkspace(id)
                    setPicker(null)
                  }}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderSubline={(p) => <span className="nc-item-sub">{p.id}</span>}
                />
              )}
            </div>
          )}
          {canPickFolder && (
            <button type="button" className="nc-browse" onClick={browse} title="Choose a folder">
              <FolderOpen size={13} weight="light" />
              Browse…
            </button>
          )}
          <span className="nc-meta nc-static">
            <House size={13} weight="light" />
            This computer
          </span>
        </div>
        {connection.state === 'error' && <p className="nc-error">{connection.error}</p>}
        {error && <p className="nc-error">{error}</p>}
        {signInFor && (
          <SignInModal
            harnessId={signInFor}
            onDone={() => setSignInFor(null)}
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
                      : `${harnessName(session.harness)} · ${session.workspace?.split(/[\\/]/).filter(Boolean).pop() ?? ''}`}
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

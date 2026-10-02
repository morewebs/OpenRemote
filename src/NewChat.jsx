import { useEffect, useState } from 'react'
import { ArrowUp, House, FolderOpen, Lightning } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { loadDefaults } from './defaults.js'
import './newchat.css'
import './composer.css'

const RECENTS_KEY = 'openremote-recent-workspaces'

function loadRecents() {
  try {
    const raw = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? '[]')
    return Array.isArray(raw) ? raw.filter((p) => typeof p === 'string').slice(0, 8) : []
  } catch {
    return []
  }
}

function saveRecents(list) {
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(list.slice(0, 8)))
  } catch {
    /* storage unavailable */
  }
}

export default function NewChat({ onOpen }) {
  const { connection, capabilities, sessions, chats, createChat, modelsFor } = useConsole()
  const [defaults] = useState(() => loadDefaults())
  const [text, setText] = useState('')
  const [workspace, setWorkspace] = useState(() => defaults.workspace ?? loadRecents()[0] ?? null)
  const [customPath, setCustomPath] = useState('')
  const [picker, setPicker] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [harness, setHarness] = useState(() => loadDefaults().harness)
  const [model, setModel] = useState(null)
  const [models, setModels] = useState([])
  const [fast, setFast] = useState(false)

  // Only what the daemon reports installed — no dead UI.
  const available = (capabilities?.harnesses ?? []).filter((h) => h.available)
  const currentHarness = available.find((h) => h.id === harness) ?? available[0] ?? null

  // The model slot: filled where the harness advertises, reserved
  // (not rendered) where it doesn't. Fast mode follows the same rule —
  // and both choices are per-harness, so switching resets them. The
  // saved defaults preselect their own harness's slots.
  useEffect(() => {
    let cancelled = false
    const defaults = loadDefaults()
    setModel(currentHarness?.id === defaults.harness ? defaults.model : null)
    setFast(false)
    if (!currentHarness) {
      setModels([])
      return
    }
    modelsFor(currentHarness.id).then((list) => {
      if (!cancelled) setModels(list ?? [])
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentHarness?.id, connection.state])

  const canSend =
    text.trim().length > 0 && workspace && !busy && currentHarness && connection.state === 'connected'
  const recents = loadRecents()
  const active = workspace ?? ''
  const inProgress = (sessions ?? []).filter((s) => ['starting', 'working', 'waiting'].includes(s.status))

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const start = async () => {
    if (!canSend) return
    setBusy(true)
    setError(null)
    try {
      const id = await createChat(text.trim(), workspace, currentHarness.id, model ?? undefined, fast || undefined)
      saveRecents([workspace, ...recents.filter((p) => p !== workspace)])
      setText('')
      onOpen(id)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  const addCustomPath = () => {
    const path = customPath.trim()
    if (!path) return
    setWorkspace(path)
    saveRecents([path, ...recents.filter((p) => p !== path)])
    setCustomPath('')
    setPicker(null)
  }

  return (
    <div className="newchat">
      <h1 className="nc-greeting">What are we working on?</h1>
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
            {models.length > 0 && (
              <>
                <button
                  className="nc-meta"
                  onClick={openPicker('model')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'model'}
                  title="Model"
                >
                  {model ?? `${currentHarness?.name} default`}
                </button>
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
              {currentHarness?.name ?? 'No harness installed'}
            </button>
            {picker?.kind === 'harness' && (
              <PickerMenu
                label="Harness"
                searchPlaceholder="Search harnesses"
                items={available.map((h) => ({ id: h.id, name: h.name }))}
                groups={null}
                selectedId={currentHarness?.id ?? ''}
                onChoose={(id) => {
                  setHarness(id)
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
                title="The harness's own fast mode — its speed tier, never a model switch"
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
          {/* First run has no recent folders — an empty picker is a dead
              control, so the button only renders when it has something
              to show; the path input is always the honest affordance. */}
          {recents.length > 0 || active ? (
            <div className="nc-device">
              {recents.length > 0 ? (
                <>
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
                </>
              ) : (
                <span className="nc-meta nc-static" title={active}>
                  <FolderOpen size={13} weight="light" />
                  {active.split(/[\\/]/).filter(Boolean).pop()}
                </span>
              )}
            </div>
          ) : null}
          <span className="nc-meta nc-static">
            <House size={13} weight="light" />
            This computer
          </span>
          <input
            className="nc-path-input"
            placeholder="Workspace folder — where the agent works, e.g. C:/Users/W/src/myapp"
            value={customPath}
            onChange={(e) => setCustomPath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                addCustomPath()
              }
            }}
            spellCheck={false}
          />
        </div>
        {error && <p className="nc-error">{error}</p>}
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

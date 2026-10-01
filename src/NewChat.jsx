import { useEffect, useState } from 'react'
import { ArrowUp, House, FolderOpen } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
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
  const [text, setText] = useState('')
  const [workspace, setWorkspace] = useState(null)
  const [customPath, setCustomPath] = useState('')
  const [picker, setPicker] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [harness, setHarness] = useState(null)
  const [model, setModel] = useState(null)
  const [models, setModels] = useState([])

  useEffect(() => {
    if (workspace == null) setWorkspace(loadRecents()[0] ?? null)
  }, [workspace])

  // Only what the daemon reports installed — no dead UI.
  const available = (capabilities?.harnesses ?? []).filter((h) => h.available)
  const currentHarness = available.find((h) => h.id === harness) ?? available[0] ?? null

  // The model slot: filled where the harness advertises, reserved
  // (not rendered) where it doesn't.
  useEffect(() => {
    let cancelled = false
    setModel(null)
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
      const id = await createChat(text.trim(), workspace, currentHarness.id, model ?? undefined)
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
                    renderTrailing={(m) =>
                      m.reasoning_efforts?.length ? (
                        <span className="nc-item-path">{m.reasoning_efforts.join(' · ')}</span>
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
          <div className="nc-device">
            <button
              className="nc-meta"
              onClick={openPicker}
              aria-haspopup="listbox"
              aria-expanded={picker != null}
              title="The folder the harness works in"
            >
              <FolderOpen size={13} weight="light" />
              {active ? active.split(/[\\/]/).filter(Boolean).pop() : 'Choose a folder'}
            </button>
            {picker != null && (
              <PickerMenu
                label="Workspace"
                searchPlaceholder="Search workspaces"
                items={recents.map((p) => ({ id: p, name: p.split(/[\\/]/).filter(Boolean).pop() }))}
                groups={null}
                selectedId={active}
                onChoose={(id) => {
                  setWorkspace(id)
                  setPicker(null)
                }}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                renderTrailing={(p) => <span className="nc-item-path">{p.id}</span>}
              />
            )}
          </div>
          <span className="nc-meta nc-static">
            <House size={13} weight="light" />
            This computer
          </span>
          <input
            className="nc-path-input"
            placeholder="…or type a folder path"
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
                      : `${HARNESS.name} · ${session.workspace?.split(/[\\/]/).filter(Boolean).pop() ?? ''}`}
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

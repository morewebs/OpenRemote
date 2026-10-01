import { useEffect, useState } from 'react'
import { ArrowUp, Asterisk, House, FolderOpen } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
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

const HARNESS = { id: 'claude', name: 'Claude Code' }

export default function NewChat({ onOpen }) {
  const { connection, sessions, chats, createChat } = useConsole()
  const [text, setText] = useState('')
  const [workspace, setWorkspace] = useState(null)
  const [customPath, setCustomPath] = useState('')
  const [picker, setPicker] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  useEffect(() => {
    if (workspace == null) setWorkspace(loadRecents()[0] ?? null)
  }, [workspace])

  const canSend = text.trim().length > 0 && workspace && !busy
  const recents = loadRecents()
  const active = workspace ?? ''
  const inProgress = (sessions ?? []).filter((s) => ['starting', 'working', 'waiting'].includes(s.status))

  const openPicker = (e) => setPicker({ x: e.clientX, y: e.clientY })

  const start = async () => {
    if (!canSend) return
    setBusy(true)
    setError(null)
    try {
      const id = await createChat(text.trim(), workspace)
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
            <span className="nc-meta nc-static" title="The one harness this build drives">
              <Asterisk size={13} weight="light" />
              {HARNESS.name}
            </span>
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

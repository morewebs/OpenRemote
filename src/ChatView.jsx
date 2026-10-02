import { useEffect, useRef, useState } from 'react'
import { ArrowUp, Lightning, Stop, Play } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { railItems } from './state/reducer.js'
import { harnessName } from './harness-names.js'
import ContextRing from './ContextRing.jsx'
import PickerMenu from './PickerMenu.jsx'
import './chatview.css'
import './composer.css'

const STATUS_LABEL = {
  starting: 'Starting',
  running: 'Running',
  waiting: 'Needs your decision',
  idle: 'Idle',
  stopped: 'Stopped',
  failed: 'Failed',
}

const TOOL_LABEL = { ok: 'Done', failed: 'Failed' }

// Codex applies model, effort, and fast on the next turn of a live thread.
// Pi applies model and thinking level on the running process. The others
// take them when the process starts, so a stopped chat can still change
// them — resume is what lands the change.
const LIVE_SETTINGS = {
  codex: { model: true, effort: true, fast: true },
  pi: { model: true, effort: true, fast: false },
}

export default function ChatView({ chat, onBack }) {
  const { capabilities, sendPrompt, updateChatSettings, answerDecision, stopChat, resumeChat, modelsFor } =
    useConsole()
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [picker, setPicker] = useState(null)
  const [models, setModels] = useState([])
  const scrollRef = useRef(null)
  const composerRef = useRef(null)
  const rail = railItems(chat)
  const pendingDecision = rail.find((item) => item.kind === 'decision' && item.pending)
  const canSend = text.trim().length > 0 && !pendingDecision && chat.running && !busy
  const harness = (capabilities?.harnesses ?? []).find((h) => h.id === chat.harness)
  const live = LIVE_SETTINGS[chat.harness] ?? { model: false, effort: false, fast: false }
  // A stopped chat can change anything the harness accepts at start. A
  // running one only where the wire accepts it now.
  const canChange = chat.running ? live : { model: true, effort: true, fast: Boolean(harness?.fast_supported) }
  const currentModel = models.find((m) => m.model === chat.model)
  const efforts = currentModel?.reasoning_efforts ?? []

  useEffect(() => {
    let cancelled = false
    if (!canChange.model && !canChange.effort) return
    modelsFor(chat.harness).then((list) => {
      if (!cancelled) setModels(list ?? [])
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chat.harness])

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const changeSettings = async (settings) => {
    setPicker(null)
    setBusy(true)
    setError(null)
    try {
      await updateChatSettings(chat.id, settings)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [rail.length, chat.status])

  // the composer floats over the transcript; its height varies as the
  // textarea grows. Publish it as --cv-clear so the thread's bottom padding
  // always keeps the last message above the box, never under it.
  useEffect(() => {
    const composer = composerRef.current
    if (!composer) return
    const set = () => {
      const root = composer.parentElement
      if (root) root.style.setProperty('--cv-clear', `${Math.ceil(composer.offsetHeight)}px`)
    }
    set()
    const ro = new ResizeObserver(set)
    ro.observe(composer)
    return () => ro.disconnect()
  }, [])

  const send = async () => {
    if (!canSend) return
    setBusy(true)
    setError(null)
    try {
      await sendPrompt(chat.id, text.trim())
      setText('')
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  const answer = async (decisionId, choice) => {
    setBusy(true)
    setError(null)
    try {
      await answerDecision(decisionId, choice)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  const control = async (action) => {
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="chatview">
      <header className="cv-head">
        <h1>{chat.title ?? 'A new task'}</h1>
        <p className="cv-facts">
          {chat.status !== 'idle' && <span className={`cv-dot cv-dot--${chat.status}`} />}
          <span className="cv-where">{chat.workspace}</span>
          <span className="cv-fact">{harnessName(chat.harness)}</span>
          {chat.model && <span className="cv-fact">{chat.model}</span>}
          {chat.effort && <span className="cv-fact">{chat.effort}</span>}
          {chat.fast && <span className="cv-fact">Fast</span>}
          {chat.approvedTools?.length > 0 && (
            <span className="cv-fact">
              {chat.approvedTools.join(' · ')} allowed for this chat
            </span>
          )}
          <span className="cv-fact">{STATUS_LABEL[chat.status] ?? chat.status}</span>
          {chat.running && (
            <button className="cv-fact-btn" onClick={() => control(() => stopChat(chat.id))} title="Stop the session">
              <Stop size={11} weight="fill" />
              Stop
            </button>
          )}
          {(chat.status === 'stopped' || chat.status === 'failed') && (
            <button className="cv-fact-btn" onClick={() => control(() => resumeChat(chat.id))} title="Resume the session">
              <Play size={11} weight="fill" />
              Resume
            </button>
          )}
        </p>
        {chat.lastError && chat.status === 'failed' && <p className="cv-facts cv-error">{chat.lastError}</p>}
      </header>
      <div className="cv-scroll" ref={scrollRef}>
        <div className="cv-thread">
          {rail.length === 0 && <p className="cv-node cv-note">The transcript will appear here.</p>}
          {rail.map((item) => {
            if (item.kind === 'message') {
              if (item.role === 'user') return <div key={item.id} className="cv-user">{item.text}</div>
              if (item.role === 'note') return <p key={item.id} className="cv-node cv-note">{item.text}</p>
              return <p key={item.id} className="cv-node cv-agent">{item.text}</p>
            }
            if (item.kind === 'tool') {
              return (
                <div key={item.id} className={`cv-node cv-node--${item.state}`}>
                  <div className="cv-tool-line">
                    <span className="cv-tool-name">{item.name}</span>
                    {item.state !== 'ok' && <span className="cv-tool-state">{TOOL_LABEL[item.state] ?? item.state}</span>}
                  </div>
                  {(item.input || item.result) && (
                    <div className="cv-io">
                      {item.input && (
                        <div className="cv-io-row">
                          <span className="cv-io-label">In</span>
                          <code>{typeof item.input === 'string' ? item.input : JSON.stringify(item.input, null, 2)}</code>
                        </div>
                      )}
                      {item.result && (
                        <div className="cv-io-row">
                          <span className="cv-io-label">Out</span>
                          <code>{item.result}</code>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              )
            }
            // decision: approval or question, options in the harness's words
            return (
              <div key={item.id} className={`cv-node cv-node--${item.pending ? 'pending' : 'done'}`}>
                <div className="cv-tool-line">
                  <span className="cv-tool-name">{item.toolName ?? 'Decision'}</span>
                  <span className="cv-tool-state">{item.pending ? 'Needs a decision' : `Answered: ${item.answeredChoice}`}</span>
                </div>
                {item.input && (
                  <div className="cv-io">
                    <div className="cv-io-row">
                      <span className="cv-io-label">In</span>
                      <code>{typeof item.input === 'string' ? item.input : JSON.stringify(item.input, null, 2)}</code>
                    </div>
                  </div>
                )}
                {item.pending && (
                  <div className="cv-tool-actions">
                    {item.options.map((option, index) => (
                      <button
                        key={option.id}
                        type="button"
                        className={index === 0 ? 'primary' : ''}
                        onClick={() => answer(item.id, option.id)}
                      >
                        {option.label}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )
          })}
          {chat.running && !pendingDecision && (
            <p className="cv-node cv-node--run cv-running">
              With {harnessName(chat.harness)} · this computer
            </p>
          )}
        </div>
      </div>

      <div className="cv-composer" ref={composerRef}>
        <div className="cv-box">
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                send()
              }
            }}
            placeholder={pendingDecision ? 'Decide above to continue' : chat.running ? 'Steer the session' : 'Resume the session to continue'}
            disabled={pendingDecision || !chat.running}
            spellCheck={false}
          />
          <div className="cv-foot">
            <div className="nc-pickers">
              <span className="nc-meta nc-static">{harnessName(chat.harness)}</span>
              {canChange.model && models.length > 0 ? (
                <button
                  type="button"
                  className="nc-meta"
                  onClick={openPicker('model')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'model'}
                  title="Model — applies to the next turn"
                >
                  {chat.model ?? 'Model'}
                </button>
              ) : (
                chat.model && <span className="nc-meta nc-static">{chat.model}</span>
              )}
              {picker?.kind === 'model' && (
                <PickerMenu
                  label="Model"
                  searchPlaceholder="Search models"
                  wide
                  items={models.map((m) => ({ id: m.model, name: m.display_name ?? m.model }))}
                  groups={null}
                  selectedId={chat.model ?? ''}
                  onChoose={(id) => changeSettings({ model: id })}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                />
              )}
              {canChange.effort && efforts.length > 0 && (
                <button
                  type="button"
                  className="nc-meta"
                  onClick={openPicker('effort')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'effort'}
                  title="The harness's own effort"
                >
                  {chat.effort ?? 'Effort'}
                </button>
              )}
              {picker?.kind === 'effort' && (
                <PickerMenu
                  label="Effort"
                  searchPlaceholder="Search efforts"
                  items={efforts.map((effort) => ({ id: effort, name: effort }))}
                  groups={null}
                  selectedId={chat.effort ?? ''}
                  onChoose={(id) => changeSettings({ effort: id })}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                />
              )}
              {canChange.fast && harness?.fast_supported && (
                <button
                  type="button"
                  className={`nc-meta nc-fast${chat.fast ? ' on' : ''}`}
                  onClick={() => changeSettings({ fast: !chat.fast })}
                  aria-pressed={Boolean(chat.fast)}
                  title="The harness's own fast mode"
                >
                  <Lightning size={13} weight={chat.fast ? 'fill' : 'light'} />
                  Fast
                </button>
              )}
              {chat.context?.window != null && (
                <ContextRing used={chat.context.used} window={chat.context.window} />
              )}
            </div>
            <div className="nc-send-group">
              <button className="nc-send" onClick={send} disabled={!canSend} title="Send">
                <ArrowUp size={15} weight="bold" />
              </button>
            </div>
          </div>
        </div>
        {error && <p className="cv-error-line">{error}</p>}
      </div>
    </div>
  )
}

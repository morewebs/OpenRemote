import { useEffect, useRef, useState } from 'react'
import { ArrowUp, Check, Lightning, Stop, Play, HandPalm, Brain, Trash, CloudArrowUp } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { chatAccess, deviceName, signedIn } from './cloud.js'
import { railItems } from './state/reducer.js'
import { harnessName } from './harness-names.js'
import { findModel, modelDisplayName } from './model-display.js'
import { HarnessMark } from './brand-marks.jsx'
import { editRows, isFileEdit, writeRows } from './diff.js'
import { renderMarkdown } from './markdown.js'
import ContextRing from './ContextRing.jsx'
import PickerMenu from './PickerMenu.jsx'
import { isMobile } from './platform.js'
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

/// The choice the harness's own words gave the answered decision - the
/// option's label, never its wire id.
const decisionAnswer = (item) =>
  item.options.find((option) => option.id === item.answeredChoice)?.label ?? item.answeredChoice ?? ''

/// The harness's own reasoning, dim and collapsible. Collapsed by
/// default; live streams breathe while the thinking runs.
function ReasoningBlock({ text, live }) {
  const [open, setOpen] = useState(false)
  const label = live ? 'Thinking…' : 'Thought process'
  return (
    <div className="cv-node cv-reasoning">
      <button type="button" className="cv-reasoning-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <Brain size={12} />
        {label}
      </button>
      {(open || live) && (
        <p
          className={`cv-reasoning-body${live ? ' live' : ''}`}
          dangerouslySetInnerHTML={{ __html: renderMarkdown(text) }}
        />
      )}
    </div>
  )
}

/// A decision, as its own card: the question is the content, the pending
/// state wears the brand amber, and the affirmative is the one amber
/// action. On answer the amber drains back to a settled card and the
/// buttons fade out over 0.3s - only for a decision this view watched
/// pending, never for history reopened already answered.
function DecisionCard({ item, onAnswer }) {
  const [leaving, setLeaving] = useState(false)
  const wasPending = useRef(item.pending)
  useEffect(() => {
    const was = wasPending.current
    wasPending.current = item.pending
    if (!was || item.pending) return
    setLeaving(true)
    const timer = setTimeout(() => setLeaving(false), 300)
    return () => clearTimeout(timer)
  }, [item.pending])
  // The question the harness actually asked, where it asked one in a
  // structured form (claude's AskUserQuestion); an approval carries its
  // content in the In row instead - never invent a question for it.
  const question =
    item.input && typeof item.input === 'object' && Array.isArray(item.input.questions)
      ? item.input.questions[0]?.question ?? null
      : null
  return (
    <div className={`cv-decision${item.pending ? ' pending' : ''}`}>
      {question && <p className="cv-decision-ask">{question}</p>}
      {item.input && !question && (
        <div className="cv-decision-io">
          <span className="cv-io-label">In</span>
          <code>{typeof item.input === 'string' ? item.input : JSON.stringify(item.input, null, 2)}</code>
        </div>
      )}
      {item.pending && item.options.length === 0 && (
        <p className="cv-decision-nochoice">
          No choices were offered - stop the session to end this turn.
        </p>
      )}
      {(item.pending || leaving) && item.options.length > 0 && (
        <div className={`cv-tool-actions${leaving && !item.pending ? ' leaving' : ''}`}>
          {item.options.map((option, index) => (
            <button
              key={option.id}
              type="button"
              className={index === 0 ? 'primary' : ''}
              onClick={() => onAnswer(item.id, option.id)}
            >
              {option.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

// Claude, Codex, and Pi apply model and effort on the running thread now
// (claude's own /model and /effort slash commands, codex thread/settings,
// pi's wire). Fast stays start-time on claude (its /fast is a toggle with
// no explicit-off form). The others take settings when the process starts,
// so a stopped chat can still change them - resume is what lands the change.
const LIVE_SETTINGS = {
  claude: { model: true, effort: true, fast: false },
  codex: { model: true, effort: true, fast: true },
  pi: { model: true, effort: true, fast: false },
}

export default function ChatView({ chat, onBack }) {
  const {
    capabilities,
    connection,
    sendPrompt,
    updateChatSettings,
    answerDecision,
    stopChat,
    interruptChat,
    resumeChat,
    modelsFor,
    devices,
    thisDevice,
    cloud,
    setDeviceKind,
    deleteChat,
    syncChat,
  } = useConsole()
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [confirmSync, setConfirmSync] = useState(false)
  // A synced chat may run on another device: what can be done from here.
  const access = chatAccess(chat, devices, thisDevice, cloud)
  const reachable = access === 'here' || access === 'remote'
  const elsewhere = chat.executor && chat.executor !== thisDevice
  const where = elsewhere ? deviceName(devices, chat.executor) : 'this computer'
  const blocked = {
    offline: `${where} is offline - this copy is read-only until it is back`,
    'not-machine': `${where} isn't a machine - make it one to continue from here`,
    'signed-out': 'Sign in to Cloud to continue this chat',
    gone: `${where} left your Cloud - this copy is read-only`,
  }[access]
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [picker, setPicker] = useState(null)
  const [models, setModels] = useState([])
  const [modelsPending, setModelsPending] = useState(true)
  const scrollRef = useRef(null)
  const composerRef = useRef(null)
  const rail = railItems(chat)
  const pendingDecision = rail.find((item) => item.kind === 'decision' && item.pending)
  // The daemon takes a prompt on any session whose driver is alive - an
  // idle chat (its last turn finished) is sendable; only stopped/failed
  // need Resume first. Gating on `running` dead-locked the composer
  // after the first turn completed.
  const alive = !['stopped', 'failed'].includes(chat.status)
  const canSend = text.trim().length > 0 && !pendingDecision && alive && !busy && reachable
  const harness = (capabilities?.harnesses ?? []).find((h) => h.id === chat.harness)
  const live = LIVE_SETTINGS[chat.harness] ?? { model: false, effort: false, fast: false }
  // A stopped chat can change anything the harness accepts at start. A
  // running one only where the wire accepts it now.
  const canChange = chat.running ? live : { model: true, effort: true, fast: Boolean(harness?.fast_supported) }
  // The catalog row for the chat's model, matched by pick word or by the
  // resolved id a running session reports back - the row either way, so
  // the chip's name never changes when the first turn resolves the id.
  const currentModel = findModel(models, chat.model)
  const efforts = currentModel?.reasoning_efforts ?? []

  useEffect(() => {
    let cancelled = false
    if (!canChange.model && !canChange.effort) {
      setModelsPending(false)
      return
    }
    setModelsPending(true)
    modelsFor(chat.harness, elsewhere ? chat.executor : null).then((list) => {
      if (!cancelled) {
        setModels(list ?? [])
        setModelsPending(false)
      }
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
          {elsewhere && <span className="cv-fact">on {where}</span>}
          {chat.model && (
            <span className="cv-fact" title={chat.model}>
              {modelDisplayName(models, chat.model)}
            </span>
          )}
          {chat.effort && <span className="cv-fact">{chat.effort}</span>}
          {chat.fast && <span className="cv-fact">Fast</span>}
          {chat.costUsd > 0 && (
            <span className="cv-fact">${chat.costUsd < 0.01 ? chat.costUsd.toFixed(4) : chat.costUsd.toFixed(2)}</span>
          )}
          {chat.thinkingTokens > 0 && (
            <span className="cv-fact">{chat.thinkingTokens.toLocaleString()} thinking tokens</span>
          )}
          {chat.approvedTools?.length > 0 && (
            <span className="cv-fact">
              {chat.approvedTools.join(' · ')} allowed for this chat
            </span>
          )}
          <span className="cv-fact">{STATUS_LABEL[chat.status] ?? chat.status}</span>
          {access === 'not-machine' && (
            <button
              className="cv-fact-btn"
              onClick={() => control(() => setDeviceKind(chat.executor, 'machine'))}
              title={`Let your other devices run chats on ${where}`}
            >
              Make {where} a machine
            </button>
          )}
          {reachable && chat.running && (
            <>
              <button className="cv-fact-btn" onClick={() => control(() => interruptChat(chat.id))} title="Interrupt the running turn - the session stays alive">
                <HandPalm size={11} weight="fill" />
                Interrupt
              </button>
              <button className="cv-fact-btn" onClick={() => control(() => stopChat(chat.id))} title="Stop the session">
                <Stop size={11} weight="fill" />
                Stop
              </button>
            </>
          )}
          {/* Resume rides the harness's own conversation (claude
              `--resume`, codex thread/resume, ...). A chat stopped before
              its first message never got one - the daemon would only
              answer "nothing to resume", so the button never shows there. */}
          {reachable && (chat.status === 'stopped' || chat.status === 'failed') && chat.resumable && (
            <button className="cv-fact-btn" onClick={() => control(() => resumeChat(chat.id))} title="Resume the session">
              <Play size={11} weight="fill" />
              Resume
            </button>
          )}
          {/* A private chat can join Cloud - asked first, since the whole
              transcript goes, tool output included. */}
          {!chat.executor && signedIn(cloud) && (
            confirmSync ? (
              <>
                <span className="cv-fact">Copy this chat, tool output included, to your other devices?</span>
                <button
                  className="cv-fact-btn"
                  onClick={() =>
                    control(async () => {
                      await syncChat(chat.id)
                      setConfirmSync(false)
                    })
                  }
                >
                  <CloudArrowUp size={11} />
                  Sync
                </button>
                <button className="cv-fact-btn" onClick={() => setConfirmSync(false)}>
                  Cancel
                </button>
              </>
            ) : (
              <button
                className="cv-fact-btn"
                onClick={() => setConfirmSync(true)}
                title="Copy this chat to your other devices"
              >
                <CloudArrowUp size={11} />
                Sync
              </button>
            )
          )}
          {/* Deleting a synced chat removes it from every device - asked
              twice, in place. */}
          {chat.executor && (
            confirmDelete ? (
              <>
                <button className="cv-fact-btn" onClick={() => control(() => deleteChat(chat.id))} title="Delete this chat on all your devices">
                  <Trash size={11} />
                  Delete everywhere
                </button>
                <button className="cv-fact-btn" onClick={() => setConfirmDelete(false)}>
                  Keep
                </button>
              </>
            ) : (
              <button className="cv-fact-btn" onClick={() => setConfirmDelete(true)} title="Delete this chat">
                <Trash size={11} />
                Delete
              </button>
            )
          )}
        </p>
        {chat.lastError && chat.status === 'failed' && <p className="cv-facts cv-error">{chat.lastError}</p>}
      </header>
      <div className="cv-scroll" ref={scrollRef}>
        <div className="cv-thread">
          {rail.length === 0 && <p className="cv-node cv-note">The transcript will appear here.</p>}
          {rail.map((item) => {
            if (item.kind === 'reasoning' || item.kind === 'reasoning-stream') {
              // The harness's own thinking, dim and collapsible - never
              // mixed into the reply. Collapsed by default; the live
              // stream shows its tail growing while it runs.
              return (
                <ReasoningBlock key={item.id} text={item.text} live={item.kind === 'reasoning-stream'} />
              )
            }
            if (item.kind === 'stream') {
              // The live reply - markdown as it arrives, with a breathing
              // caret riding the end. The settled message.added replaces
              // this item whole.
              return (
                <p
                  key={item.id}
                  className="cv-node cv-agent"
                  dangerouslySetInnerHTML={{ __html: `${renderMarkdown(item.text)}<span class="cv-caret"></span>` }}
                />
              )
            }
            if (item.kind === 'message') {
              if (item.role === 'user') return <div key={item.id} className="cv-user">{item.text}</div>
              if (item.role === 'note') return <p key={item.id} className="cv-node cv-note">{item.text}</p>
              // Agent messages are markdown - the harness's own output
              // shape - rendered through the escape-first mini renderer.
              return (
                <p
                  key={item.id}
                  className="cv-node cv-agent"
                  dangerouslySetInnerHTML={{ __html: renderMarkdown(item.text) }}
                />
              )
            }
            if (item.kind === 'tool') {
              // A file edit renders as a split diff - the tool card's
              // honest shape, never a JSON dump of its input.
              const edit = isFileEdit(item.input)
                ? item.input.old_string != null
                  ? editRows(item.input.old_string, item.input.new_string)
                  : writeRows(item.input.content)
                : null
              // The status rides the name line, right-aligned - the beat of
              // agent work: a breathing green dot and Running while the tool
              // is in flight, a check the moment the result lands (green
              // for done, red for failed) - and it stays, the transcript's
              // own record. A tool left unresolved by an interrupted turn
              // never pretends: no status word.
              const inFlight = item.result == null && chat.running
              return (
                <div key={item.id} className={`cv-node cv-node--${item.state}`}>
                  <div className="cv-tool-line">
                    <span className="cv-tool-name">{item.name}</span>
                    {inFlight ? (
                      <span className="cv-tool-status">
                        <span className="cv-tool-status-dot" />
                        Running
                      </span>
                    ) : item.state === 'failed' ? (
                      <span className="cv-tool-status">
                        <Check className="cv-check--failed" size={13} weight="bold" />
                        Failed
                      </span>
                    ) : (
                      <span className="cv-tool-status">
                        <Check className="cv-check--ok" size={13} weight="bold" />
                        Done
                      </span>
                    )}
                  </div>
                  {edit && edit.length > 0 && (
                    <div className="cv-diff">
                      <div className="cv-diff-file">{item.input.file_path}</div>
                      <div className="cv-diff-grid">
                        {edit.map((row, i) => (
                          <div className="cv-diff-row" key={i}>
                            <span className={`cv-diff-side cv-diff-side--${row.left.kind ?? 'empty'}`}>{row.left.text}</span>
                            <span className={`cv-diff-side cv-diff-side--${row.right.kind ?? 'empty'}`}>{row.right.text}</span>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                  {(item.input || item.result) && !edit && (
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
                  <span className="cv-tool-state">
                    {item.pending
                      ? 'Needs a decision'
                      : `Answered: ${decisionAnswer(item)}`}
                  </span>
                </div>
                <DecisionCard item={item} onAnswer={answer} />
              </div>
            )
          })}
          {chat.running && !pendingDecision && (
            <p className="cv-node cv-node--run cv-running">
              With {harnessName(chat.harness)} · {where}
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
              // A phone's keyboard Enter is a new line; the send button sends.
              if (e.key === 'Enter' && !e.shiftKey && !isMobile) {
                e.preventDefault()
                send()
              }
            }}
            placeholder={
              blocked ??
              (pendingDecision
                ? 'Decide above to continue'
                : alive
                  ? chat.running
                    ? 'Steer the session'
                    : 'Send a message'
                  : chat.resumable
                    ? 'Resume the session to continue'
                    : 'This chat never started')
            }
            disabled={pendingDecision || !alive || !reachable}
            spellCheck={false}
          />
          <div className="cv-foot">
            <div className="nc-pickers">
              {modelsPending && !chat.model ? (
                <span className="nc-skeleton" aria-hidden="true" />
              ) : canChange.model && models.length > 0 ? (
                <button
                  type="button"
                  className="nc-meta"
                  onClick={openPicker('model')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'model'}
                  title="Model - applies to the next turn"
                >
                  {chat.model ? modelDisplayName(models, chat.model) : 'Model'}
                </button>
              ) : (
                chat.model && (
                  <span className="nc-meta nc-static" title={chat.model}>
                    {modelDisplayName(models, chat.model)}
                  </span>
                )
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
              {(canChange.model ? models.length > 0 : Boolean(chat.model)) && (
                <span className="nc-via">via</span>
              )}
              <span className="nc-meta nc-static">
                <HarnessMark harness={chat.harness} size={16} />
                {harnessName(chat.harness)}
              </span>
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
            </div>
            <div className="nc-send-group">
              {/* The ring renders wherever usage is known - the composer's
                  one live element; a claude chat carries it from its first
                  breath at zero. Codex reports its window too; claude
                  reports usage only, so the fill rides a nominal window
                  and the card says so. */}
              {(chat.context?.used != null || chat.harness === 'claude') && (
                <ContextRing
                  used={chat.context?.used ?? 0}
                  window={chat.context?.window ?? (chat.harness === 'claude' ? 200_000 : 0)}
                  nominal={chat.context?.window == null}
                />
              )}
              <button className="nc-send" onClick={send} disabled={!canSend} title="Send">
                <ArrowUp size={15} weight="bold" />
              </button>
            </div>
          </div>
        </div>
        {(error || connection.state === 'error') && (
          <p className="cv-error-line">{error ?? connection.error}</p>
        )}
      </div>
    </div>
  )
}

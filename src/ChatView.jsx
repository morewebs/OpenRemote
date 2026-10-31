import { useEffect, useRef, useState } from 'react'
import {
  ArrowUp,
  ArrowUpRight,
  Check,
  Stop,
  Play,
  HandPalm,
  Brain,
  Trash,
  CloudArrowUp,
  PencilSimple,
  X,
} from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { chatAccess, deviceName, signedIn } from './cloud.js'
import { railItems } from './state/reducer.js'
import { quietRail, toolHint, workIsLive, workLabel } from './quiet-rail.js'
import { harnessName } from './harness-names.js'
import { findModel, modelDisplayName, prettyModelName } from './model-display.js'
import { HarnessMark } from './brand-marks.jsx'
import { pickable } from './harness-manager.js'
import HarnessManagerModal from './HarnessManagerModal.jsx'
import SignInModal from './SignInModal.jsx'
import { editRows, isFileEdit, writeRows } from './diff.js'
import { isCommandRun } from './command-card.js'
import { renderMarkdown } from './markdown.js'
import { bodyPreview, capRows, MAX_RENDER_LINES, MAX_RENDER_ROWS, shouldRenderLive } from './render-budget.js'
import { jsonCaption, jsonIsLarge, jsonShape, readJson } from './json-card.js'
import ContextRing from './ContextRing.jsx'
import PickerMenu from './PickerMenu.jsx'
import { EffortSlider, FadingLabel, effortLabel } from './NewChat.jsx'
import { EndpointManagerModal } from './EndpointManager.jsx'
import { isMobile } from './platform.js'
import { usePrivacyMode } from './privacy.js'
import { useHint, useModelSwitches, usePaletteSource, useShortcut } from './keyboard.jsx'
import { useComposerMenus } from './useComposerMenus.jsx'
import { mentionedChats, ownCommands, takeCommands } from './composer-menus.js'
import { claimInserter } from './composer-insert.js'
import './chatview.css'
import './composer.css'

// The harnesses that run one process per prompt cannot take a message
// mid-turn: Steer interrupts the turn and runs the message next there.
const INTERRUPT_STEER = new Set(['grok', 'agy'])

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

/// One markdown body, priced: a settled body's text never changes again,
/// so it parses once and the cache stands through every later rail render
/// - a stream delta used to re-parse every earlier message. A live body
/// (a token stream) re-parses at most once every 100ms; between beats the
/// last HTML stands, and React sees the same string and skips the
/// innerHTML write, so a fast token beat neither re-parses nor rewrites
/// the growing reply per token. The cache keys on the text, not the item
/// object: the reducer folds deltas into the live item in place, so item
/// identity would freeze while the text grew - a changed text re-parses,
/// an unchanged one reuses the cached HTML.
function MarkdownBody({ text, live = false, className, tail = '' }) {
  const cache = useRef({ text: null, html: '', at: 0 })
  let html = cache.current.html
  if (cache.current.text !== text && (!live || shouldRenderLive(cache.current.at))) {
    html = renderMarkdown(text)
    cache.current = { text, html, at: Date.now() }
  }
  return <p className={className} dangerouslySetInnerHTML={{ __html: html + tail }} />
}

/// The harness's own reasoning, dim and collapsible. The pill is the
/// whole mark while it runs and after it settles. The words stay hidden
/// until the reader asks - a live stream was a wall of text.
/// One quiet row for a turn's thinking and tool calls. The conversation
/// stays the user message and the reply. The steps open as names, not
/// as a log of every JSON body.
// Work row for a turn. Named at module scope so the rail can render it.
function WorkCluster({ items, running }) {
  const [open, setOpen] = useState(false)
  const live = workIsLive(items, running)
  const tools = items.filter((item) => item.kind === 'tool')
  return (
    <div className="cv-node cv-work">
      <button type="button" className="cv-reasoning-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        {live && <span className="cv-tool-status-dot" />}
        {workLabel(items, live)}
      </button>
      {open && tools.length > 0 && (
        <ul className="cv-work-list">
          {tools.map((item) => (
            <li key={item.id}>
              <span className="cv-work-name">{item.name}</span>
              {toolHint(item) && <span className="cv-work-hint pv">{toolHint(item)}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function ReasoningBlock({ text, live }) {
  const [open, setOpen] = useState(false)
  const label = live ? 'Thinking…' : 'Thought process'
  return (
    <div className="cv-node cv-reasoning">
      <button type="button" className="cv-reasoning-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <Brain size={12} />
        {label}
      </button>
      {open && (
        <MarkdownBody text={text} live={live} className={`cv-reasoning-body pv${live ? ' live' : ''}`} />
      )}
    </div>
  )
}

/// In or Out on a tool card. JSON is fields (a url, an id, a count), not
/// a `{ "key": "..." }` dump. A long string stays a size mark. Prose that
/// is short stays readable.
function ToolIo({ label, raw }) {
  const parsed = readJson(raw)
  if (parsed) return <JsonPane label={label} value={parsed} />
  return <TextIo label={label} raw={typeof raw === 'string' ? raw : JSON.stringify(raw)} />
}

function JsonPane({ label, value }) {
  const [open, setOpen] = useState(false)
  if (!jsonIsLarge(value)) {
    return (
      <div className="cv-io-row">
        <span className="cv-io-label">{label}</span>
        <JsonView value={value} />
      </div>
    )
  }
  const preview = bodyPreview(typeof value === 'string' ? value : JSON.stringify(value))
  return (
    <>
      <div className="cv-io-row">
        <span className="cv-io-label">{label}</span>
        <button type="button" className="cv-io-fold" onClick={() => setOpen(!open)} aria-expanded={open}>
          <span className="cv-io-meter" style={{ '--fill': preview.fill }} aria-hidden="true" />
          <span>{open ? 'Hide' : jsonCaption(value)}</span>
        </button>
      </div>
      {open && (
        <div className="cv-io-open">
          <JsonView value={value} />
        </div>
      )}
    </>
  )
}

function JsonView({ value }) {
  return <ShapeView shape={jsonShape(value)} />
}

function ShapeView({ shape, mono = false }) {
  if (!shape) return null
  if (shape.kind === 'object') {
    return (
      <div className="cv-json">
        {shape.fields.map((field) => (
          <div className="cv-json-row" key={field.key}>
            <span className="cv-json-key">{field.key}</span>
            <ShapeView shape={field.shape} mono={field.mono} />
          </div>
        ))}
        {shape.more > 0 && <div className="cv-io-more">{shape.more} more fields</div>}
      </div>
    )
  }
  if (shape.kind === 'list') {
    const noun = shape.count === 1 ? '1 item' : `${shape.count} items`
    return (
      <div className="cv-json">
        <span className="cv-json-count">{noun}</span>
        {shape.items.map((item, index) => (
          <ShapeView key={index} shape={item} />
        ))}
        {shape.more > 0 && <div className="cv-io-more">{shape.more} more items</div>}
      </div>
    )
  }
  if (shape.kind === 'url') {
    return <span className="cv-json-val cv-json-val--mono pv" title={shape.href}>{shape.label}</span>
  }
  if (shape.kind === 'folded') {
    return <FoldedText shape={shape} />
  }
  return <span className={`cv-json-val pv${mono ? ' cv-json-val--mono' : ''}`}>{shape.text}</span>
}

function FoldedText({ shape }) {
  const [open, setOpen] = useState(false)
  return (
    <span className="cv-json-fold">
      <button type="button" className="cv-io-fold" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="cv-io-meter" style={{ '--fill': shape.fill }} aria-hidden="true" />
        <span>{open ? 'Hide' : shape.size}</span>
      </button>
      {open && <code className="pv">{shape.text}</code>}
    </span>
  )
}

function TextIo({ label, raw }) {
  const preview = bodyPreview(raw)
  const [open, setOpen] = useState(false)
  if (!preview.large) {
    return (
      <div className="cv-io-row">
        <span className="cv-io-label">{label}</span>
        <code className="pv">{preview.text}</code>
      </div>
    )
  }
  return (
    <>
      <div className="cv-io-row">
        <span className="cv-io-label">{label}</span>
        <button type="button" className="cv-io-fold" onClick={() => setOpen(!open)} aria-expanded={open}>
          <span className="cv-io-meter" style={{ '--fill': preview.fill }} aria-hidden="true" />
          <span>{open ? 'Hide' : preview.size}</span>
        </button>
      </div>
      {open && (
        <div className="cv-io-open">
          <code className="pv">{preview.text}</code>
          {preview.hidden > 0 && (
            <div className="cv-io-more">Showing the first {MAX_RENDER_LINES} lines.</div>
          )}
        </div>
      )}
    </>
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
  // content as a terminal tile instead - the command it wants to run,
  // never a JSON dump.
  const question =
    item.input && typeof item.input === 'object' && Array.isArray(item.input.questions)
      ? item.input.questions[0]?.question ?? null
      : null
  const command = !question && isCommandRun(item.input) ? item.input : null
  // An approval's input rides its tool card once the answer settles it -
  // the run card is the one full record, so the settled card keeps the
  // answer line alone (never the same command twice). The 0.3s fade
  // holds the tile through the settle; a question card always keeps its
  // body - nothing else carries the question.
  const keepBody = item.pending || leaving || Boolean(question)
  return (
    <div className={`cv-decision${item.pending ? ' pending' : ''}`}>
      {question && <p className="cv-decision-ask pv">{question}</p>}
      {keepBody && command && (
        <div className="cv-cmd pv">
          {command.description && (
            <span className="cv-cmd-desc">{command.description}</span>
          )}
          <code className="cv-cmd-text">{command.command}</code>
        </div>
      )}
      {keepBody && item.input && !question && !command && (
        <div className="cv-decision-io pv">
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
              {/* a question's options are the user's content; an
                  approval's words are the harness's, and stay readable */}
              {question ? <span className="pv">{option.label}</span> : option.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

// Claude, Codex, Pi, and OpenCode apply a model on the running thread
// (claude's own /model, codex thread/settings, pi's wire, OpenCode's
// session model route). Fast stays start-time on claude (its /fast is a
// toggle with no explicit-off form). Grok and Antigravity take a model
// when the process starts; an idle chat still changes it, and the daemon
// restarts that process onto the new model. A turn that is still working
// waits.
const LIVE_SETTINGS = {
  claude: { model: true, effort: true, fast: false },
  codex: { model: true, effort: true, fast: true },
  pi: { model: true, effort: true, fast: false },
  opencode: { model: true, effort: false, fast: false },
}

export default function ChatView({ chat, onBack, focused = true }) {
  const privacy = usePrivacyMode()
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
    endpointsFor,
    endpointModelsFor,
    setChatEndpoint,
    devices,
    thisDevice,
    cloud,
    setDeviceKind,
    deleteChat,
    syncChat,
    queueMessage,
    removeQueued,
    steerQueued,
    steerMessage,
    resumeQueue,
    renameChat,
    chats,
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
  const [pickerLeaving, setPickerLeaving] = useState(false)
  const pickerLeavingRef = useRef(false)
  const [effortLeaving, setEffortLeaving] = useState(false)
  const effortLeavingRef = useRef(false)
  const [models, setModels] = useState([])
  const [modelsPending, setModelsPending] = useState(true)
  // Custom endpoints on the machine running this chat: the records and
  // what each enabled one lists today. A stopped chat can switch between
  // them - resume lands the change; a live chat keeps its binding.
  const [endpoints, setEndpoints] = useState([])
  const [endpointModels, setEndpointModels] = useState([])
  const [manageModels, setManageModels] = useState(false)
  const [managingHarnesses, setManagingHarnesses] = useState(false)
  const [signInFor, setSignInFor] = useState(null)
  const [customDraft, setCustomDraft] = useState('')
  const scrollRef = useRef(null)
  const composerRef = useRef(null)
  const inputRef = useRef(null)
  const rail = railItems(chat)
  const quiet = quietRail(rail)
  const pendingDecision = rail.find((item) => item.kind === 'decision' && item.pending)
  // Every tool call riding the rail, keyed the way the duplicate checks
  // pair a decision with its run: (name, input). A settled approval
  // whose run is here hides - the run card is the one full record.
  const toolCalls = new Set(
    rail.filter((item) => item.kind === 'tool').map((item) => `${item.name}|${JSON.stringify(item.input)}`),
  )
  // The daemon takes a prompt on any session whose driver is alive - an
  // idle chat (its last turn finished) is sendable; only stopped/failed
  // need Resume first. Gating on `running` dead-locked the composer
  // after the first turn completed.
  const alive = !['stopped', 'failed'].includes(chat.status)
  // While a turn is working (or waiting on a decision) a message queues in
  // the daemon and runs as its own turn when this one ends.
  const turnOpen = chat.status === 'running' || chat.status === 'waiting'
  const queue = chat.queue ?? []
  const canSend = text.trim().length > 0 && alive && !busy && reachable
  const harness = (capabilities?.harnesses ?? []).find((h) => h.id === chat.harness)
  const live = LIVE_SETTINGS[chat.harness] ?? { model: false, effort: false, fast: false }
  // A stopped chat can change anything the harness accepts at start. A
  // running one only where the wire accepts it now - permission never:
  // every harness takes the word at process start only, so a running
  // chat shows the word as a fact, a stopped one can change it.
  const canChange = chat.running
    ? { ...live, permission: false }
    : { model: true, effort: true, fast: Boolean(harness?.fast_supported), permission: true }
  // The catalog row for the chat's model, matched by pick word or by the
  // resolved id a running session reports back - the row either way, so
  // the chip's name never changes when the first turn resolves the id.
  const currentModel = findModel(models, chat.model)
  const efforts = currentModel?.reasoning_efforts ?? []
  // The picker's custom rows, grouped under each endpoint's name. A
  // composite id carries the endpoint back out (model ids never hold a
  // newline - the daemon rejects control characters).
  const customItems = endpointModels.map((m) => ({
    id: `${m.endpoint_id}\n${m.model}`,
    name: prettyModelName(m.display_name ?? m.model),
    group: m.endpoint_name,
    endpointId: m.endpoint_id,
    model: m.model,
  }))
  const modelItems = [
    ...models.map((m) => ({
      id: m.model,
      name: prettyModelName(m.display_name ?? m.model),
      resolved_model: m.resolved_model,
    })),
    ...customItems,
  ]
  const endpointById = new Map(endpoints.map((e) => [e.id, e]))
  const chatEndpointName = chat.endpoint_id ? (endpointById.get(chat.endpoint_id)?.name ?? 'a custom endpoint') : null
  const defaultEndpoint = endpoints.find((e) => e.is_default) ?? null
  const canSwitchEndpoint = !chat.running && reachable && (harness?.custom_endpoint_supported ?? false)
  const reloadEndpoints = async () => {
    if (!harness?.custom_endpoint_supported) return
    const rows = (await endpointsFor(chat.harness, elsewhere ? chat.executor : null).catch(() => [])) ?? []
    setEndpoints(rows)
    const live = rows.filter((e) => e.enabled)
    if (!live.length) {
      setEndpointModels([])
      return
    }
    const lists = await Promise.all(
      live.map((e) => endpointModelsFor(e.id, elsewhere ? chat.executor : null).catch(() => [])),
    )
    setEndpointModels(lists.flat())
  }
  // The / and @ menus, asking the machine that runs this chat.
  const cloudReady = signedIn(cloud)
  const menus = useComposerMenus({
    text,
    setText,
    inputRef,
    harness: chat.harness,
    sessionId: chat.id,
    deviceId: elsewhere ? chat.executor : null,
    inChat: true,
    cloudReady,
    enabled: alive && reachable,
    choicesFor: (arg) => {
      if (arg === 'model') return modelItems.map((m) => ({ id: m.id, name: m.name }))
      if (arg === 'effort') return efforts.map((e) => ({ id: e }))
      if (arg === 'fast') return [{ id: 'on' }, { id: 'off' }]
      if (arg === 'permission') return (harness?.permission_modes ?? []).map((w) => ({ id: w.w, name: w.l }))
      return []
    },
  })

  // The browser pane's "attach to the chat" lands here: appended to the
  // draft, then the focus comes back so the user keeps writing. Claimed
  // again on focus, so the focused composer wins when panes split.
  const attach = (snippet) => {
    setText((t) => (t ? `${t}\n\n${snippet}` : snippet))
    requestAnimationFrame(() => inputRef.current?.focus())
  }
  useEffect(() => {
    claimInserter(attach)
  }, [])

  // An OpenRemote command from the composer, run here before the rest of
  // the message goes: the same paths the pickers and buttons take.
  const runCommand = async ({ name, arg }) => {
    if (name === 'model' || name === 'effort' || name === 'permissions') {
      if (!arg) throw new Error(`/${name} needs a choice - pick one from its menu`)
      const key = { model: 'model', effort: 'effort', permissions: 'permissionMode' }[name]
      await updateChatSettings(chat.id, { [key]: arg })
    } else if (name === 'fast') {
      await updateChatSettings(chat.id, { fast: arg ? arg.toLowerCase() === 'on' : !chat.fast })
    } else if (name === 'stop') {
      await stopChat(chat.id)
    } else if (name === 'resume') {
      await resumeChat(chat.id)
    } else if (name === 'rename') {
      if (!arg) throw new Error('/rename needs a name: /rename "New name"')
      await renameChat(chat.id, arg)
    } else if (name === 'sync' || name === 'cloud') {
      await syncChat(chat.id)
    }
  }

  // The message as it goes: OpenRemote's commands run and leave it; what
  // remains goes to the harness with the chats it points at.
  const prepare = async () => {
    const { run, text: message } = takeCommands(text.trim(), ownCommands({ inChat: true, cloud: cloudReady }))
    for (const command of run) await runCommand(command)
    return { message, pointed: mentionedChats(message, chats) }
  }

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
    if (harness?.custom_endpoint_supported) {
      endpointsFor(chat.harness, elsewhere ? chat.executor : null).then((rows) => {
        if (cancelled) return
        setEndpoints(rows ?? [])
        const live = (rows ?? []).filter((e) => e.enabled)
        if (!live.length) {
          setEndpointModels([])
          return
        }
        Promise.all(
          live.map((e) => endpointModelsFor(e.id, elsewhere ? chat.executor : null).catch(() => [])),
        ).then((lists) => {
          if (cancelled) return
          setEndpointModels(lists.flat())
        })
      }).catch(() => {
        if (!cancelled) {
          setEndpoints([])
          setEndpointModels([])
        }
      })
    } else {
      setEndpoints([])
      setEndpointModels([])
    }
    return () => {
      cancelled = true
    }
    // The fetch also follows the connection and the executor: one failed
    // call (a restarting daemon, a machine briefly offline) used to stick
    // an empty list for the view's lifetime, and a chat that synced to a
    // machine kept the catalog of where it ran before. An empty answer is
    // not cached (modelsFor), so a retry is cheap.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chat.harness, connection.state, elsewhere ? chat.executor : null])

  const boxSpot = (edge = 'left') => {
    const rect = composerRef.current?.querySelector('.cv-box')?.getBoundingClientRect()
    if (!rect || !(rect.width > 0)) return { x: 16, y: 16 }
    const x = edge === 'right' ? Math.round(rect.right - 264) : Math.round(rect.left)
    return { x, y: Math.round(rect.top) }
  }

  const hidePicker = () => {
    if (!picker || pickerLeavingRef.current) return
    pickerLeavingRef.current = true
    setPickerLeaving(true)
    setTimeout(() => {
      pickerLeavingRef.current = false
      setPickerLeaving(false)
      setPicker(null)
    }, 160)
  }

  const togglePicker = (kind, edge = 'left') => () => {
    if (picker?.kind === kind) {
      hidePicker()
      return
    }
    if (pickerLeavingRef.current) return
    const spot = boxSpot(edge)
    setPickerLeaving(false)
    setPicker({ kind, x: spot.x, y: spot.y })
  }

  const leaveEffort = () => {
    if (effortLeavingRef.current) return
    effortLeavingRef.current = true
    setEffortLeaving(true)
    setTimeout(() => {
      effortLeavingRef.current = false
      setEffortLeaving(false)
      setPicker((current) => (current?.kind === 'effort' ? null : current))
    }, 160)
  }

  const toggleEffort = () => {
    if (picker?.kind === 'effort') leaveEffort()
    else if (!effortLeavingRef.current) setPicker({ kind: 'effort', x: 1, y: 1 })
  }

  const applySettings = async (settings) => {
    setError(null)
    try {
      await updateChatSettings(chat.id, settings)
    } catch (err) {
      setError(err.message ?? String(err))
    }
  }

  useEffect(() => {
    if (picker?.kind !== 'effort') return
    const close = (e) => {
      if (e.target.closest?.('.nc-effort-wrap, .nc-effort')) return
      leaveEffort()
    }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [picker?.kind])

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

  // A stopped chat's endpoint switch lands on resume; a live chat
  // refuses with 409 - the chip only opens where a switch can land.
  const changeEndpoint = async (endpointId) => {
    setPicker(null)
    setBusy(true)
    setError(null)
    try {
      await setChatEndpoint(chat.id, endpointId || null, elsewhere ? chat.executor : null)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  const chooseModelId = (id) => {
    const custom = customItems.find((m) => m.id === id)
    if (custom) {
      // A custom row implies its endpoint - but only where a switch
      // can land. On a live chat the model still changes; the endpoint
      // stays the binding it started with and resume picks it up.
      if (canSwitchEndpoint && custom.endpointId !== (chat.endpoint_id ?? null)) {
        changeEndpoint(custom.endpointId).then(() => changeSettings({ model: custom.model }))
      } else {
        changeSettings({ model: custom.model })
      }
    } else {
      changeSettings({ model: id })
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
      const { message, pointed } = await prepare()
      if (message) {
        if (turnOpen) await queueMessage(chat.id, message, pointed)
        else await sendPrompt(chat.id, message, pointed)
      }
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

  // The keyboard acts on the focused pane's chat (keyboard.jsx). A key
  // whose action can't happen here - no running turn to interrupt, no
  // model list - falls through untouched.
  const openPickerAt = (kind) => {
    const el = composerRef.current?.querySelector(`[data-picker="${kind}"]`)
    if (!el) return false
    if (picker?.kind === kind) {
      hidePicker()
      return true
    }
    const spot = boxSpot(kind === 'permission' ? 'right' : 'left')
    setPicker({ kind, x: spot.x, y: spot.y })
    return true
  }
  const permissionWords = harness?.permission_modes ?? []
  const canPickPermission = permissionWords.length > 0 && canChange.permission && !chat.running
  useShortcut(
    'focus-composer',
    () => {
      const el = inputRef.current
      if (!el || el.disabled) return false
      el.focus()
    },
    focused,
  )
  useShortcut(
    'interrupt',
    () => {
      if (!reachable || !chat.running) return false
      control(() => interruptChat(chat.id))
    },
    focused,
  )
  useShortcut(
    'stop',
    () => {
      if (!reachable || !chat.running) return false
      control(() => stopChat(chat.id))
    },
    focused,
  )
  useShortcut('pick-model', () => openPickerAt('model'), focused)
  useShortcut(
    'pick-effort',
    () => {
      if (!canChange.effort || efforts.length === 0) return false
      toggleEffort()
    },
    focused,
  )
  useShortcut('pick-permission', () => openPickerAt('permission'), focused)
  useShortcut(
    'toggle-fast',
    () => {
      if (!canChange.fast || !harness?.fast_supported) return false
      applySettings({ fast: !chat.fast })
    },
    focused,
  )
  useModelSwitches((i) => {
    if (!canChange.model || !models[i]) return false
    changeSettings({ model: models[i].model })
  }, focused)
  usePaletteSource(
    'model',
    () =>
      canChange.model && models.length
        ? {
            title: 'Switch model',
            items: models.map((m) => ({
              id: m.model,
              name: prettyModelName(m.display_name ?? m.model),
              icon: null,
            })),
            selectedId: currentModel?.model ?? chat.model,
            choose: (id) => changeSettings({ model: id }),
          }
        : null,
    focused,
  )
  usePaletteSource(
    'effort',
    () =>
      canChange.effort && efforts.length
        ? {
            title: 'Set effort',
            items: efforts.map((e) => ({ id: e, name: e })),
            selectedId: chat.effort,
            choose: (id) => changeSettings({ effort: id }),
          }
        : null,
    focused,
  )
  usePaletteSource(
    'permission',
    () =>
      canPickPermission
        ? {
            title: 'Set permission',
            items: permissionWords.map((w) => ({ id: w.w, name: w.l })),
            selectedId: chat.permissionMode,
            choose: (id) => changeSettings({ permissionMode: id }),
          }
        : null,
    focused,
  )
  // Steer (Ctrl+Enter): into the running turn now; with nothing running
  // it is an ordinary send.
  const steerNow = async () => {
    if (!text.trim() || !alive || !reachable || busy) return false
    if (!turnOpen) {
      send()
      return
    }
    setBusy(true)
    setError(null)
    try {
      const { message, pointed } = await prepare()
      if (message) await steerMessage(chat.id, message, pointed)
      setText('')
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }
  useShortcut('steer', steerNow, focused)
  // Edit: a queued message comes back to the composer (in front of
  // anything already typed) and leaves the queue; Enter queues it again.
  const pullBack = (message) => {
    setText((typed) => (typed.trim() ? `${message.text}\n${typed}` : message.text))
    control(() => removeQueued(chat.id, message.id))
    inputRef.current?.focus()
  }
  const steerLabel = turnOpen
    ? INTERRUPT_STEER.has(chat.harness)
      ? 'Interrupt + send'
      : 'Steer into the running turn'
    : 'Send now'
  // The row's Steer button says what it will do: steer, interrupt and
  // send (one process per prompt), or simply send when nothing runs.
  const steerButton = turnOpen ? (INTERRUPT_STEER.has(chat.harness) ? 'Interrupt + send' : 'Steer') : 'Send now'
  const hintSteer = useHint('steer')
  const hintInterrupt = useHint('interrupt')
  const hintStop = useHint('stop')
  const hintModel = useHint('pick-model')
  const hintEffort = useHint('pick-effort')
  const hintPermission = useHint('pick-permission')

  return (
    <div className="chatview">
      <header className="cv-head">
        <h1 className="pv">{chat.title ?? 'A new task'}</h1>
        <p className="cv-facts">
          {chat.status !== 'idle' && <span className={`cv-dot cv-dot--${chat.status}`} />}
          <span className="cv-where pv">{chat.workspace}</span>
          <span className="cv-fact">{harnessName(chat.harness)}</span>
          {elsewhere && <span className="cv-fact">on <span className="pv">{where}</span></span>}
          {chat.model && (
            <span className="cv-fact" title={chat.model}>
              {modelDisplayName(models, chat.model)}
            </span>
          )}
          {chat.effort && <span className="cv-fact">{chat.effort}</span>}
          {chat.permissionMode && (
            <span
              className="cv-fact"
              title={`The harness's permission word for this chat (${harnessName(chat.harness)}'s own spelling)`}
            >
              {chat.permissionMode}
            </span>
          )}
          {chat.fast && <span className="cv-fact">Fast</span>}
          {chat.costUsd > 0 && (
            <span className="cv-fact">${chat.costUsd < 0.01 ? chat.costUsd.toFixed(4) : chat.costUsd.toFixed(2)}</span>
          )}
          {chat.thinkingTokens > 0 && (
            <span className="cv-fact">{chat.thinkingTokens.toLocaleString()} thinking tokens</span>
          )}
          {chat.approvedTools?.length > 0 && (
            <span className="cv-fact">
              <span className="pv">{chat.approvedTools.join(' · ')}</span> allowed for this chat
            </span>
          )}
          <span className="cv-fact">{STATUS_LABEL[chat.status] ?? chat.status}</span>
          {access === 'not-machine' && (
            <button
              className="cv-fact-btn"
              onClick={() => control(() => setDeviceKind(chat.executor, 'machine'))}
              title={privacy ? 'Let your other devices run chats on it' : `Let your other devices run chats on ${where}`}
            >
              Make <span className="pv">{where}</span> a machine
            </button>
          )}
          {reachable && chat.running && (
            <>
              <button className="cv-fact-btn" onClick={() => control(() => interruptChat(chat.id))} title={hintInterrupt('Interrupt the running turn - the session stays alive')}>
                <HandPalm size={11} weight="fill" />
                Interrupt
              </button>
              <button className="cv-fact-btn" onClick={() => control(() => stopChat(chat.id))} title={hintStop('Stop the session')}>
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
        {chat.lastError && chat.status === 'failed' && <p className="cv-facts cv-error pv">{chat.lastError}</p>}
      </header>
      <div className="cv-scroll" ref={scrollRef}>
        <div className="cv-thread">
          {rail.length === 0 && <p className="cv-node cv-note">The transcript will appear here.</p>}
          {quiet.map((item) => {
            if (item.kind === 'work') {
              return <WorkCluster key={item.id} items={item.items} running={chat.running} />
            }
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
              // this item whole, so a beat the throttle holds back never
              // loses text: the settle carries the complete reply.
              return (
                <MarkdownBody
                  key={item.id}
                  text={item.text}
                  live
                  className="cv-node cv-agent pv"
                  tail='<span class="cv-caret"></span>'
                />
              )
            }
            if (item.kind === 'message') {
              if (item.role === 'user') return <div key={item.id} className="cv-user pv">{item.text}</div>
              if (item.role === 'note') return <p key={item.id} className="cv-node cv-note pv">{item.text}</p>
              // Agent messages are markdown - the harness's own output
              // shape - rendered through the escape-first mini renderer,
              // parsed once per message (MarkdownBody's cache).
              return <MarkdownBody key={item.id} text={item.text} className="cv-node cv-agent pv" />
            }
            if (item.kind === 'tool') {
              // While the ask for this very call is pending, the decision
              // card is the one card - it carries the command and the
              // buttons, and this one saying "Running" would be a lie (the
              // call is blocked) and a duplicate. It returns once the
              // answer lands and the run is real.
              const isThePendingAsk =
                pendingDecision &&
                pendingDecision.toolName === item.name &&
                JSON.stringify(pendingDecision.input) === JSON.stringify(item.input)
              if (isThePendingAsk) return null
              // A file edit renders as a split diff - the tool card's
              // honest shape, never a JSON dump of its input.
              const edit = isFileEdit(item.input)
                ? item.input.old_string != null
                  ? editRows(item.input.old_string, item.input.new_string)
                  : writeRows(item.input.content)
                : null
              // What the card actually shows: a bounded first slice, never
              // the whole walk - a 3000-line write renders 500 rows plus a
              // note, not thousands of DOM nodes. Tool inputs and outputs
              // take the same cap on their lines.
              const diff = edit && edit.length > 0 ? capRows(edit) : null
              // A command run renders as a terminal tile - the command
              // itself in mono, its description as the caption, never a
              // JSON dump of the input. The tile replaces the In row
              // alone; the Out row still shows what the command printed.
              const command = !edit && isCommandRun(item.input) ? item.input : null
              const shownIn = item.input && !command && !edit ? item.input : null
              const shownOut = item.result ? item.result : null
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
                  {diff && (
                    <div className="cv-diff pv">
                      <div className="cv-diff-file">{item.input.file_path}</div>
                      <div className="cv-diff-grid">
                        {diff.rows.map((row, i) => (
                          <div className="cv-diff-row" key={i}>
                            <span className={`cv-diff-side cv-diff-side--${row.left.kind ?? 'empty'}`}>{row.left.text}</span>
                            <span className={`cv-diff-side cv-diff-side--${row.right.kind ?? 'empty'}`}>{row.right.text}</span>
                          </div>
                        ))}
                        {diff.hidden > 0 && (
                          <div className="cv-diff-more">Showing the first {MAX_RENDER_ROWS} rows.</div>
                        )}
                      </div>
                    </div>
                  )}
                  {command && (
                    <div className="cv-cmd pv">
                      {/* the description leads - what this is, in plain
                          words - then the command itself in mono */}
                      {command.description && (
                        <span className="cv-cmd-desc">{command.description}</span>
                      )}
                      <code className="cv-cmd-text">{command.command}</code>
                    </div>
                  )}
                  {(shownIn || shownOut) && !edit && (
                    <div className="cv-io pv">
                      {shownIn && <ToolIo label="In" raw={shownIn} />}
                      {shownOut && <ToolIo label="Out" raw={shownOut} />}
                    </div>
                  )}
                </div>
              )
            }
            // decision: approval or question, options in the harness's words.
            // A settled approval whose own run rides the rail hides -
            // the run card is the one full record (its Done/Failed is
            // the outcome); the answered card would only repeat the
            // command. Questions always keep theirs - nothing else
            // carries the Q&A - and a settled approval with no run card
            // (an ask that never became one) keeps its answer line.
            const isQuestion =
              item.input && typeof item.input === 'object' && Array.isArray(item.input.questions)
            if (
              !item.pending &&
              !isQuestion &&
              item.toolName &&
              toolCalls.has(`${item.toolName}|${JSON.stringify(item.input)}`)
            ) {
              return null
            }
            return (
              <div key={item.id} className={`cv-node cv-node--${item.pending ? 'pending' : 'done'}`}>
                <div className="cv-tool-line">
                  <span className="cv-tool-name">{item.toolName ?? 'Decision'}</span>
                  <span className="cv-tool-state">
                    {item.pending ? (
                      'Needs a decision'
                    ) : isQuestion ? (
                      <>Answered: <span className="pv">{decisionAnswer(item)}</span></>
                    ) : (
                      `Answered: ${decisionAnswer(item)}`
                    )}
                  </span>
                </div>
                <DecisionCard item={item} onAnswer={answer} />
              </div>
            )
          })}
          {chat.running && !pendingDecision && !quiet.some((item) => item.kind === 'work' && workIsLive(item.items, true)) && !quiet.some((item) => item.kind === 'stream') && (
            <p className="cv-node cv-node--run cv-running">
              Working...
            </p>
          )}
        </div>
      </div>

      <div className="cv-composer" ref={composerRef}>
        <div className="cv-box">
          {menus.element}
          {queue.length > 0 && (
            <div className="cv-queue">
              <div className="cv-queue-head">
                {chat.queuePaused ? (
                  <>
                    <span>Queue paused</span>
                    <button
                      type="button"
                      className="cv-queue-resume"
                      disabled={!reachable}
                      onClick={() => control(() => resumeQueue(chat.id))}
                    >
                      Resume
                    </button>
                  </>
                ) : (
                  <span>{queue.length} queued</span>
                )}
              </div>
              {queue.map((message) => (
                <div key={message.id} className="cv-queue-row">
                  <span className="cv-queue-text pv">{message.text}</span>
                  <span className="cv-queue-actions">
                    <button
                      type="button"
                      className="cv-queue-icon"
                      title="Edit - back to the composer"
                      onClick={() => pullBack(message)}
                    >
                      <PencilSimple size={16} />
                    </button>
                    <button
                      type="button"
                      className="cv-queue-icon"
                      title="Remove"
                      onClick={() => control(() => removeQueued(chat.id, message.id))}
                    >
                      <X size={16} />
                    </button>
                    <button
                      type="button"
                      className="cv-queue-steer"
                      title={hintSteer(steerLabel)}
                      disabled={!reachable}
                      onClick={() => control(() => steerQueued(chat.id, message.id))}
                    >
                      <ArrowUpRight size={14} weight="bold" />
                      {steerButton}
                    </button>
                  </span>
                </div>
              ))}
            </div>
          )}
          <textarea
            ref={inputRef}
            value={text}
            onFocus={() => claimInserter(attach)}
            onChange={(e) => {
              setText(e.target.value)
              menus.track(e)
            }}
            onKeyUp={menus.track}
            onClick={menus.track}
            onKeyDown={(e) => {
              // The / and @ menus take their keys first.
              if (menus.onKeyDown(e)) return
              // A phone's keyboard Enter is a new line; the send button sends.
              // Ctrl/Cmd+Enter is Steer's (the keyboard catalog's).
              if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && !isMobile) {
                e.preventDefault()
                send()
              }
              // Up on an empty composer brings the last queued message back.
              if (e.key === 'ArrowUp' && !text && queue.length > 0 && !isMobile) {
                e.preventDefault()
                pullBack(queue[queue.length - 1])
              }
            }}
            placeholder={
              blocked ??
              (alive
                ? pendingDecision
                  ? 'Decide above - or queue a message'
                  : turnOpen
                    ? 'Queue a message'
                    : 'Send a message'
                : chat.resumable
                  ? 'Resume the session to continue'
                  : 'This chat never started')
            }
            disabled={!alive || !reachable}
            spellCheck={false}
          />
          <div className="cv-foot">
            <div className="nc-pickers">
              {modelsPending && !chat.model ? (
                <span className="nc-skeleton" aria-hidden="true" />
              ) : canChange.model ? (
                <button
                  type="button"
                  className="nc-meta"
                  onClick={togglePicker('model')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'model'}
                  data-picker="model"
                  data-unset={!chat.model || undefined}
                  title={hintModel('Model - applies to the next turn')}
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
                  items={modelItems}
                  groups={customItems.length > 0}
                  selectedId={chat.model ?? ''}
                  onChoose={chooseModelId}
                  leaving={pickerLeaving}
                  place="above"
                  onClose={hidePicker}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderSubline={(m) =>
                    m.group ? <span className="nc-item-sub">via {m.group}</span> : null
                  }
                  footer={
                    harness?.custom_endpoint_supported ? (
                      <button
                        type="button"
                        onClick={() => {
                          setPicker(null)
                          setManageModels(true)
                        }}
                      >
                        Manage models…
                      </button>
                    ) : null
                  }
                />
              )}
              {(canChange.model ? modelItems.length > 0 : Boolean(chat.model)) && (
                <span className="nc-via">via</span>
              )}
              {harness?.custom_endpoint_supported && (chat.endpoint_id || endpoints.length > 0) && (
                canSwitchEndpoint ? (
                  <button
                    type="button"
                    className="nc-meta"
                    onClick={togglePicker('endpoint')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'endpoint'}
                    data-picker="endpoint"
                    title="Which model server this chat runs through - lands when the chat next starts"
                  >
                    {chatEndpointName ?? (defaultEndpoint ? `Default (${defaultEndpoint.name})` : 'Harness provider')}
                  </button>
                ) : (
                  chatEndpointName && (
                    <span className="nc-meta nc-static" title="The model server this chat runs through">
                      via {chatEndpointName}
                    </span>
                  )
                )
              )}
              {picker?.kind === 'endpoint' && canSwitchEndpoint && (
                <PickerMenu
                  label="Endpoint"
                  searchPlaceholder="Search endpoints"
                  items={[
                    {
                      id: '',
                      name: defaultEndpoint ? `Harness default (${defaultEndpoint.name})` : 'Harness provider',
                    },
                    ...endpoints.filter((e) => e.enabled).map((e) => ({ id: e.id, name: e.name })),
                  ]}
                  groups={null}
                  selectedId={chat.endpoint_id ?? ''}
                  onChoose={(id) => changeEndpoint(id)}
                  leaving={pickerLeaving}
                  place="above"
                  onClose={hidePicker}
                  anchor={{ left: picker.x, top: picker.y }}
                  footer={
                    <button
                      type="button"
                      onClick={() => {
                        setPicker(null)
                        setManageModels(true)
                      }}
                    >
                      Manage models…
                    </button>
                  }
                />
              )}
              <button
                type="button"
                className="nc-meta"
                onClick={togglePicker('harness')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'harness'}
                data-picker="harness"
                title="Harness"
              >
                <HarnessMark harness={chat.harness} size={16} />
                {harnessName(chat.harness)}
              </button>
              {picker?.kind === 'harness' && (
                <PickerMenu
                  label="Harness"
                  searchPlaceholder="Search harnesses"
                  leaving={pickerLeaving}
                  place="above"
                  items={(capabilities?.harnesses ?? []).filter(pickable).map((h) => ({ id: h.id, name: h.name }))}
                  renderIcon={(h) => <HarnessMark harness={h.id} size={16} />}
                  groups={null}
                  selectedId={chat.harness}
                  onChoose={() => hidePicker()}
                  onClose={hidePicker}
                  anchor={{ left: picker.x, top: picker.y }}
                  footer={
                    isMobile ? null : (
                      <button
                        type="button"
                        onClick={() => {
                          setPicker(null)
                          setManagingHarnesses(true)
                        }}
                      >
                        Manage harnesses…
                      </button>
                    )
                  }
                />
              )}
              {canChange.effort && efforts.length > 0 && (
                <div className="nc-effort-wrap">
                  <button
                    type="button"
                    className="nc-meta"
                    onClick={toggleEffort}
                    aria-haspopup="dialog"
                    aria-expanded={picker?.kind === 'effort'}
                    data-picker="effort"
                    title={hintEffort("The harness's own effort")}
                  >
                    <FadingLabel text={effortLabel(chat.effort)} />
                  </button>
                  {(picker?.kind === 'effort' || effortLeaving) && (
                    <EffortSlider
                      tiers={efforts}
                      value={chat.effort}
                      leaving={effortLeaving}
                      place="above"
                      spot={(() => {
                        const rect = composerRef.current?.querySelector('.cv-box')?.getBoundingClientRect()
                        if (!rect || !(rect.width > 0)) return null
                        return { left: Math.round(rect.left), top: Math.round(rect.top) }
                      })()}
                      fast={Boolean(chat.fast)}
                      fastSupported={canChange.fast && harness?.fast_supported === true}
                      onEffort={(tier) => applySettings({ effort: tier })}
                      onFast={() => applySettings({ fast: !chat.fast })}
                    />
                  )}
                </div>
              )}

            </div>
            <div className="nc-send-group">
              {/* The harness's own permission word for this chat, by the
                  send button: the last thing to check before a message
                  goes. It is start-time on every harness - the chip
                  changes it only on a stopped chat (resume lands it); on a
                  running one it stays the quiet fact the facts row already
                  shows. */}
              {(harness?.permission_modes ?? []).length > 0 &&
                (canChange.permission && !chat.running ? (
                  <button
                    type="button"
                    className="nc-meta"
                    data-unset={!chat.permissionMode || undefined}
                    onClick={togglePicker('permission', 'right')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'permission'}
                    data-picker="permission"
                    title={hintPermission('What the harness may do without asking - lands when the chat next starts')}
                  >
                    {chat.permissionMode ?? 'Permission'}
                  </button>
                ) : (
                  chat.permissionMode && (
                    <span
                      className="nc-meta nc-static"
                      title={`The harness's permission word for this chat (${harnessName(chat.harness)}'s own spelling)`}
                    >
                      {chat.permissionMode}
                    </span>
                  )
                ))}
              {picker?.kind === 'permission' && (
                <PickerMenu
                  label="Permission"
                  searchPlaceholder="Search permission words"
                  items={(harness?.permission_modes ?? []).map((word) => ({
                    id: word.w,
                    name: word.l,
                  }))}
                  groups={null}
                  selectedId={chat.permissionMode ?? ''}
                  onChoose={(id) => changeSettings({ permissionMode: id })}
                  leaving={pickerLeaving}
                  place="above"
                  onClose={hidePicker}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderSubline={(item) =>
                    item.id === harness?.default_permission ? (
                      <span className="nc-item-sub">
                        {harnessName(chat.harness)} runs this by default
                      </span>
                    ) : null
                  }
                />
              )}
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
              <button
                className="nc-send"
                onClick={send}
                disabled={!canSend}
                title={turnOpen ? `Queue (Enter) - ${hintSteer(steerLabel)}` : 'Send'}
              >
                <ArrowUp size={15} weight="bold" />
              </button>
            </div>
          </div>
        </div>
        {(error || connection.state === 'error') && (
          <p className="cv-error-line">{error ?? connection.error}</p>
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
        {signInFor && <SignInModal harnessId={signInFor} onDone={() => setSignInFor(null)} />}
        {manageModels && (
          <EndpointManagerModal
            harness={chat.harness}
            deviceId={elsewhere ? chat.executor : null}
            where={elsewhere ? where : 'this computer'}
            onClose={() => setManageModels(false)}
            onChanged={reloadEndpoints}
          />
        )}
      </div>
    </div>
  )
}

// The JavaScript plugin runtime, the console's half. An enabled JS plugin's
// pages run in frames this module makes - a hidden one for a plugin that
// puts buttons in the composer, one for each open view - and every call a
// frame makes is answered here (plugin-calls.js), checked first against the
// install record the user approved: its powers and its contributions, never
// the plugin's files as they are now.
//
// A frame is sandboxed to scripts alone (FRAME_SANDBOX), so it can read
// nothing of the console - not its storage, not the daemon token - and
// reaches it only by postMessage. A message counts only from a frame this
// module made: the very window, with the opaque origin every such frame has
// (frameOf). The frame never gets the token: the console makes each daemon
// request itself and hands back only the answer.

import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { isMac, useKeyboard } from './keyboard.jsx'
import { CATALOG, actionForCombo, comboFromEvent } from './shortcuts.js'
import { addAttachment } from './attachments.js'
import {
  FRAME_SANDBOX,
  contributionsOf,
  displayName,
  frameAllow,
  frameDocument,
  frameOf,
  keyPassesUp,
  parseViewKey,
  ridesProject,
  viewKey,
  withDefaults,
} from './plugin-runtime.js'
import { answerCall, composerGrants, dropGrants, grantClick, makeCalls, settingsView } from './plugin-calls.js'
import './plugin-ui.css'

const PluginContext = createContext(null)

// The app's colors and type, handed to every frame as CSS variables of the
// same names, so a plugin's page can wear the app's look.
const THEME_TOKENS = [
  '--bg-app',
  '--bg-side',
  '--bg-hover',
  '--bg-active',
  '--line',
  '--text-1',
  '--text-2',
  '--text-3',
  '--accent',
  '--green',
  '--dot-idle',
  '--radius',
  '--font-sans',
]

const TOAST_MS = 6000
const MAX_TOASTS = 3

const reduceMotionNow = () =>
  typeof document !== 'undefined' &&
  (document.documentElement.hasAttribute('data-reduce-motion') ||
    window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true)

function themeTokens() {
  const style = getComputedStyle(document.documentElement)
  return Object.fromEntries(THEME_TOKENS.map((name) => [name, style.getPropertyValue(name).trim()]).filter(([, v]) => v))
}

const BACKGROUND = { kind: 'background', view: null }

export function PluginProvider({ children }) {
  const { api, plugins } = useConsole()
  const keyboard = useKeyboard()
  const [inputStates, setInputStates] = useState({})
  // Plugins whose hidden frame is up: each one whose composer button was
  // clicked once. A plugin costs nothing until then.
  const [awake, setAwake] = useState(() => new Set())
  const [toasts, setToasts] = useState([])

  const framesRef = useRef(new Map())
  const grantsRef = useRef(composerGrants())
  const clicksRef = useRef([])
  const navRef = useRef({ openView: null, openSettings: null })
  const toastSeq = useRef(0)

  // Every frame boots from the same small page (the policy and the bridge);
  // the bridge then asks for the plugin's own.
  const frameSrc = useMemo(
    () => URL.createObjectURL(new Blob([frameDocument(window.location.origin)], { type: 'text/html' })),
    [],
  )

  const installed = plugins?.installed ?? null

  // Each installed plugin: its record, what it contributes (from the
  // record), its name as people read it. Only a JavaScript plugin's pages
  // run here; every plugin's settings are set here.
  const allRecords = useMemo(
    () => (installed ?? []).map((plugin) => ({ plugin, ui: contributionsOf(plugin), name: displayName(plugin) })),
    [installed],
  )
  const records = useMemo(
    () => new Map(allRecords.filter((r) => r.plugin.runtime === 'js').map((r) => [r.plugin.id, r])),
    [allRecords],
  )

  // The ones that run here: enabled.
  const running = useMemo(() => [...records.values()].filter((r) => r.plugin.enabled), [records])

  // Every view and composer button, each with its plugin's record so a
  // surface can keep a project's plugin to that project's chats.
  const views = useMemo(
    () =>
      running.flatMap((r) =>
        r.ui.views.map((v) => ({
          key: viewKey(r.plugin.id, v.id),
          pluginId: r.plugin.id,
          plugin: r.plugin,
          viewId: v.id,
          title: v.title,
          icon: v.icon,
          pluginName: r.name,
        })),
      ),
    [running],
  )

  const inputs = useMemo(
    () => running.flatMap((r) => r.ui.inputs.map((input) => ({ pluginId: r.plugin.id, plugin: r.plugin, pluginName: r.name, input }))),
    [running],
  )

  // Settings belong to every installed plugin that declares them, on or
  // off: a plugin is set up before it is turned on as often as after.
  const settingsPlugins = useMemo(() => allRecords.filter((r) => r.ui.settings.length > 0), [allRecords])

  const backgroundIds = useMemo(
    () => running.filter((r) => awake.has(r.plugin.id) && r.ui.inputs.length > 0).map((r) => r.plugin.id),
    [running, awake],
  )

  // A plugin turned off or removed: its frames go (they unmount with their
  // records), and so do its composer grants and button states.
  useEffect(() => {
    const live = new Set(running.map((r) => r.plugin.id))
    dropGrants(grantsRef.current, live)
    setInputStates((current) => {
      const keys = Object.keys(current).filter((k) => !live.has(k.slice(0, k.lastIndexOf('/'))))
      if (!keys.length) return current
      const next = { ...current }
      for (const k of keys) delete next[k]
      return next
    })
    setAwake((current) => {
      const kept = [...current].filter((id) => live.has(id))
      return kept.length === current.size ? current : new Set(kept)
    })
  }, [running])

  // ---- notes in the app ----

  const toast = useCallback((pluginName, text) => {
    const id = ++toastSeq.current
    setToasts((current) => [...current, { id, pluginName, text }].slice(-MAX_TOASTS))
    setTimeout(() => setToasts((current) => current.filter((t) => t.id !== id)), TOAST_MS)
  }, [])

  // ---- frames ----

  const post = (frame, message, transfer) => {
    try {
      frame.win.postMessage({ openremote: 1, ...message }, '*', transfer ?? [])
    } catch {
      /* the frame went away mid-answer */
    }
  }

  const postEvent = (frame, type, detail) => {
    if (!frame.booted) frame.queue.push({ type, detail })
    else post(frame, { kind: 'event', type, detail })
  }

  // Clicks wait for their plugin's hidden frame: it comes up on the first
  // click and hears every click from then on.
  const deliverClicks = () => {
    const waiting = []
    for (const click of clicksRef.current) {
      const frame = [...framesRef.current.values()].find(
        (f) => f.pluginId === click.pluginId && f.place.kind === 'background',
      )
      if (frame) postEvent(frame, 'composer.click', click.detail)
      else waiting.push(click)
    }
    clicksRef.current = waiting
  }

  const registerFrame = useCallback((iframe, { pluginId, place }) => {
    const win = iframe.contentWindow
    if (!win) return undefined
    const frame = { win, el: iframe, pluginId, place, booted: false, queue: [] }
    framesRef.current.set(win, frame)
    if (place.kind === 'background') deliverClicks()
    return () => {
      if (framesRef.current.get(win) === frame) framesRef.current.delete(win)
    }
    // deliverClicks reads refs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The newest console state, for answers that arrive after a render.
  const live = useRef({})
  live.current = { api, records, toast, bindings: keyboard?.bindings ?? {} }

  const callsRef = useRef(null)
  if (!callsRef.current) {
    callsRef.current = makeCalls({
      api: () => live.current.api,
      toast: (name, text) => live.current.toast(name, text),
      openView: (key) => navRef.current.openView?.(key),
      openSettings: (pluginId) => navRef.current.openSettings?.(pluginId),
      setInputState: (key, state) => setInputStates((current) => ({ ...current, [key]: state })),
      attach: (item, key) => addAttachment(item, key),
      grants: grantsRef.current,
    })
  }

  const boot = (frame) => {
    const record = live.current.records.get(frame.pluginId)
    if (!record || !record.plugin.enabled) return
    const entry =
      frame.place.kind === 'view'
        ? record.ui.views.find((v) => v.id === frame.place.view)?.entry
        : record.ui.inputs[0]?.entry ?? record.ui.entry
    post(frame, {
      kind: 'boot',
      plugin: {
        id: record.plugin.id,
        name: record.name,
        version: record.plugin.version ?? null,
        powers: record.plugin.powers ?? [],
      },
      place: frame.place,
      entry: entry ? { path: entry } : null,
      error: entry ? null : `${record.name} names no page for this place.`,
      theme: themeTokens(),
      reduceMotion: reduceMotionNow(),
    })
  }

  // The one listener for every frame's messages.
  const onMessageRef = useRef(null)
  onMessageRef.current = async (event) => {
    const frame = frameOf(event, framesRef.current)
    if (!frame) return
    const message = event.data
    const record = live.current.records.get(frame.pluginId)
    const who = record?.name ?? frame.pluginId
    if (message.kind === 'ready') {
      // A page that says hello is a fresh one (the first, or the plugin
      // reloaded its frame): it boots, and events wait for it again. A
      // second hello from the same page meets a bridge that boots once.
      frame.booted = false
      boot(frame)
      return
    }
    if (message.kind === 'booted') {
      frame.booted = true
      const queued = frame.queue.splice(0)
      for (const item of queued) post(frame, { kind: 'event', type: item.type, detail: item.detail })
      return
    }
    if (message.kind === 'log') {
      console.warn(`[plugin ${who}] ${String(message.text ?? '').slice(0, 500)}`)
      return
    }
    if (message.kind === 'failed') {
      // A view shows its own problem; a hidden page's would go unseen.
      const words = String(message.text ?? '').slice(0, 200)
      console.warn(`[plugin ${who}] ${words}`)
      if (frame.place.kind === 'background') live.current.toast(who, `It could not start: ${words}`)
      return
    }
    if (message.kind === 'key') {
      // The app's keys keep working from a focused plugin view: the ones
      // that move around the app, and only while the view has the focus.
      const combo = comboFromEvent(
        {
          key: String(message.key ?? ''),
          code: String(message.code ?? ''),
          ctrlKey: Boolean(message.ctrlKey),
          metaKey: Boolean(message.metaKey),
          altKey: Boolean(message.altKey),
          shiftKey: Boolean(message.shiftKey),
        },
        isMac,
      )
      const actionId = combo ? actionForCombo(live.current.bindings, combo) : null
      const action = CATALOG.find((a) => a.id === actionId)
      const focusedHere = typeof document !== 'undefined' && document.activeElement === frame.el
      if (!keyPassesUp(action, { view: frame.place.kind === 'view', focused: focusedHere })) return
      window.dispatchEvent(
        new KeyboardEvent('keydown', {
          key: String(message.key ?? '').slice(0, 32),
          code: String(message.code ?? '').slice(0, 32),
          ctrlKey: Boolean(message.ctrlKey),
          metaKey: Boolean(message.metaKey),
          altKey: Boolean(message.altKey),
          shiftKey: Boolean(message.shiftKey),
          repeat: Boolean(message.repeat),
          bubbles: true,
          cancelable: true,
        }),
      )
      return
    }
    // A call: answered only as the record allows, and with its own result.
    const reply = await answerCall(record, message, callsRef.current)
    const { transfer, ...payload } = reply
    post(frame, { kind: 'reply', id: message.id, ...payload }, transfer)
  }

  useEffect(() => {
    const onMessage = (event) => onMessageRef.current?.(event)
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [])

  // The app's reduce-motion switch reaches every frame as it changes.
  useEffect(() => {
    const observer = new MutationObserver(() => {
      const reduce = reduceMotionNow()
      for (const frame of framesRef.current.values()) postEvent(frame, 'motion', { reduce })
    })
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-reduce-motion'] })
    return () => observer.disconnect()
    // postEvent reads refs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // ---- what the composers, the sidebar, the panes and Settings use ----

  const registerComposer = useCallback((handle, entry) => {
    const composers = grantsRef.current.composers
    composers.set(handle, entry)
    return () => {
      if (composers.get(handle) === entry) composers.delete(handle)
    }
  }, [])

  // The user's click on a plugin's button: the one thing that lets the
  // plugin write in that composer.
  const clickInput = useCallback((pluginId, inputId, handle) => {
    grantClick(grantsRef.current, pluginId, handle)
    clicksRef.current.push({ pluginId, detail: { input: inputId, composer: handle } })
    setAwake((current) => (current.has(pluginId) ? current : new Set([...current, pluginId])))
    deliverClicks()
    // deliverClicks reads refs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // A button's state in one composer: the plugin's, when it set it for
  // every composer, for this one, or for this chat's composer before it
  // opened again; at rest otherwise.
  const inputState = useCallback(
    (pluginId, inputId, handle, composerKey) => {
      const state = inputStates[`${pluginId}/${inputId}`]
      const mine =
        state &&
        (!state.composer || state.composer === handle || (state.composerKey != null && state.composerKey === composerKey))
      return mine ? state : { state: 'idle', label: null, title: null }
    },
    [inputStates],
  )

  // The views and buttons for chats of one project (null: none): a Global
  // or Machine plugin's everywhere, a project's plugin's only there.
  const viewsFor = useCallback((projectId) => views.filter((v) => ridesProject(v.plugin, projectId)), [views])
  const inputsFor = useCallback((projectId) => inputs.filter((i) => ridesProject(i.plugin, projectId)), [inputs])

  const setNavigation = useCallback((nav) => {
    navRef.current = nav
  }, [])

  const openSettings = useCallback((pluginId) => navRef.current.openSettings?.(pluginId), [])

  const viewInfo = useCallback(
    (key) => {
      const parsed = parseViewKey(key)
      if (!parsed) return null
      const record = records.get(parsed.pluginId) ?? null
      const view = record?.ui.views.find((v) => v.id === parsed.viewId) ?? null
      return { ...parsed, record, view, plugin: installed?.find((p) => p.id === parsed.pluginId) ?? null }
    },
    [records, installed],
  )

  // Settings saved from the form reach the plugin's frames too - values
  // and whether each secret is set, never a secret itself.
  const saveSettings = useCallback(
    async (pluginId, values) => {
      const view = settingsView(await api.savePluginSettings(pluginId, values))
      const read = withDefaults(view, live.current.records.get(pluginId)?.ui.settings ?? [])
      for (const frame of framesRef.current.values()) {
        if (frame.pluginId === pluginId) postEvent(frame, 'settings.changed', read)
      }
      return view
    },
    // postEvent reads refs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [api],
  )

  const loadSettings = useCallback(async (pluginId) => settingsView(await api.pluginSettings(pluginId)), [api])

  const value = useMemo(
    () => ({
      frameSrc,
      records,
      views,
      inputs,
      viewsFor,
      inputsFor,
      settingsPlugins,
      registerFrame,
      registerComposer,
      clickInput,
      inputState,
      setNavigation,
      openSettings,
      viewInfo,
      loadSettings,
      saveSettings,
    }),
    [frameSrc, records, views, inputs, viewsFor, inputsFor, settingsPlugins, registerFrame, registerComposer, clickInput, inputState, setNavigation, openSettings, viewInfo, loadSettings, saveSettings],
  )

  return (
    <PluginContext.Provider value={value}>
      {children}
      {/* Hidden frames: the pages of plugins that add composer buttons, up
          from their first click. */}
      <div className="plg-hidden" aria-hidden="true">
        {backgroundIds.map((id) => {
          const record = records.get(id)
          return (
            <PluginFrame
              key={`${id}|${record?.plugin.updated_at ?? ''}`}
              pluginId={id}
              place={BACKGROUND}
              title={`${record?.name ?? id} (background)`}
            />
          )
        })}
      </div>
      {toasts.length > 0 && (
        <div className="plg-toasts" role="status" aria-live="polite">
          {toasts.map((t) => (
            <div className="plg-toast" key={t.id}>
              <span className="plg-toast-from">{t.pluginName}</span>
              <span className="plg-toast-text">{t.text}</span>
              <button
                type="button"
                className="plg-toast-close"
                title="Dismiss"
                aria-label="Dismiss"
                onClick={() => setToasts((current) => current.filter((x) => x.id !== t.id))}
              >
                <X size={12} weight="bold" />
              </button>
            </div>
          ))}
        </div>
      )}
    </PluginContext.Provider>
  )
}

export function usePlugins() {
  return useContext(PluginContext)
}

// One plugin page in a frame: sandboxed to scripts alone (never the same
// origin), the microphone allowed only for a plugin given it.
export function PluginFrame({ pluginId, place, title, className = 'plg-frame' }) {
  const host = useContext(PluginContext)
  const record = host?.records.get(pluginId)
  if (!host || !record) return null
  return (
    <FrameElement
      host={host}
      pluginId={pluginId}
      kind={place.kind}
      view={place.view ?? null}
      allow={frameAllow(record.plugin.powers)}
      title={title}
      className={className}
    />
  )
}

// The frame itself, noted with the host before its page can speak: a
// layout effect runs before the browser gets a turn to load the frame.
function FrameElement({ host, pluginId, kind, view, allow, title, className }) {
  const ref = useRef(null)
  const { registerFrame } = host
  useLayoutEffect(() => {
    if (!ref.current) return undefined
    return registerFrame(ref.current, { pluginId, place: { kind, view } })
  }, [registerFrame, pluginId, kind, view])
  return (
    <iframe
      ref={ref}
      className={className}
      title={title}
      src={host.frameSrc}
      sandbox={FRAME_SANDBOX}
      allow={allow}
      referrerPolicy="no-referrer"
    />
  )
}

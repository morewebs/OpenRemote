// The keyboard layer: one window listener turns a keydown into an action
// from the catalog (shortcuts.js) and runs whoever handles it right now.
// Handlers register where they live - the shell for navigation and
// layout, the sidebar for its own list, the focused pane's chat or New
// chat for its composer and pickers - so an action always means "here".
//
// Shortcuts are a desktop thing: a phone registers nothing.

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react'
import {
  actionForCombo,
  blocksWhileTyping,
  browserBindings,
  clearStaleOverrides,
  comboFromEvent,
  comboLabel,
  effectiveBindings,
  firesWhileTyping,
  loadOverrides,
  rebind,
  resetOne,
  saveOverrides,
  staleOverrides,
  typesIn,
  unbind,
} from './shortcuts.js'
import { isMobile } from './platform.js'

export const isMac =
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || '')

// While one of these is open it owns the keyboard: Esc closes it, the
// arrows move inside it, and no app shortcut fires underneath.
const OVERLAY = '[role="dialog"], .nc-menu, .ctx-menu'

/// Whether an overlay is really on screen. The selector alone is not enough: a menu
/// left in the DOM but hidden (display: none, a hidden attribute) still matches, and
/// then every shortcut in the app would be dead with nothing to say why. A rendered
/// node has client rects; a hidden one has none.
function overlayOpen() {
  const node = document.querySelector(OVERLAY)
  return Boolean(node && node.getClientRects().length > 0)
}

const KeyboardContext = createContext(null)

export function KeyboardProvider({ children }) {
  const [overrides, setOverrides] = useState(loadOverrides)
  // The app's keys, which this layer dispatches, and the browser's, which
  // the browser dispatches itself while it has focus (BrowserView). Lists
  // and labels read both through `bindings`; the ids never overlap.
  const appBindings = useMemo(() => effectiveBindings(overrides, isMac), [overrides])
  const browserKeys = useMemo(() => browserBindings(overrides, isMac), [overrides])
  const bindings = useMemo(() => ({ ...appBindings, ...browserKeys }), [appBindings, browserKeys])
  const bindingsRef = useRef(appBindings)
  bindingsRef.current = appBindings
  const handlers = useRef(new Map())
  const sources = useRef(new Map())
  // The Keyboard settings' "press the new keys" moment: every shortcut
  // stands down so the keys land as a binding, not an action.
  const capturing = useRef(false)

  const register = useCallback((map, id, entry) => {
    const list = map.get(id) ?? []
    list.push(entry)
    map.set(id, list)
    return () => {
      const current = map.get(id) ?? []
      const i = current.indexOf(entry)
      if (i !== -1) current.splice(i, 1)
    }
  }, [])

  // The newest enabled registration wins: two chats in two panes both
  // register, and only the focused one is enabled.
  const live = useCallback((map, id) => {
    const list = map.get(id) ?? []
    for (let i = list.length - 1; i >= 0; i--) if (list[i].enabled.current) return list[i].fn.current
    return null
  }, [])

  // Runs an action; false when nobody can take it here (the key then
  // keeps its ordinary meaning).
  const run = useCallback(
    (id) => {
      const fn = live(handlers.current, id)
      return fn ? fn() !== false : false
    },
    [live],
  )

  const available = useCallback((id) => Boolean(live(handlers.current, id)), [live])
  const source = useCallback((kind) => live(sources.current, kind)?.() ?? null, [live])

  useEffect(() => {
    if (isMobile) return
    const onKey = (e) => {
      if (capturing.current || e.defaultPrevented) return
      const combo = comboFromEvent(e, isMac)
      if (!combo) return
      const id = actionForCombo(bindingsRef.current, combo)
      if (!id) return
      if (overlayOpen()) return
      const typing = typesIn(document.activeElement)
      if (typing && !firesWhileTyping(combo)) return
      // An action that leaves the chat does not fire while the caret is in a
      // field: the draft lives in the composer's view, and the shell cannot
      // save text it cannot read, so the navigation would unmount it. A Mod
      // combo is still swallowed here, so the browser never opens its own
      // window in the app's place.
      if (typing && blocksWhileTyping(id)) {
        if (/(^|\+)(Mod|Ctrl)\+/.test(combo)) e.preventDefault()
        return
      }
      // Esc is the one action that fires while typing, because holding a turn is
      // the chat's own gesture and the composer is inside the chat. From any other
      // field Esc is the field's key (clear it, then leave it), and those handlers
      // do not stop the event, so without this an Escape pressed to clear the
      // sidebar's search also aborted the turn the sidebar was showing.
      if (typing && id === 'interrupt' && !document.activeElement?.closest?.('.chatview')) return
      // A Ctrl/Cmd key the app owns never falls through to the webview's
      // own accelerators (Ctrl+P would print) even when nothing can act on
      // it here; Esc and Alt keys do fall through, keeping their meaning.
      if (run(id) || /(^|\+)(Mod|Ctrl)\+/.test(combo)) e.preventDefault()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [run])

  const commit = useCallback((next) => {
    saveOverrides(next)
    setOverrides(next)
  }, [])

  const value = useMemo(
    () => ({
      bindings,
      browserBindings: browserKeys,
      overrides,
      label: (id) => comboLabel(bindings[id], isMac),
      run,
      available,
      source,
      registerHandler: (id, entry) => register(handlers.current, id, entry),
      registerSource: (kind, entry) => register(sources.current, kind, entry),
      setCapturing: (on) => {
        capturing.current = on
      },
      rebind: (id, combo) => {
        const { overrides: next, displaced } = rebind(overrides, id, combo, isMac)
        commit(next)
        return displaced
      },
      unbind: (id) => commit(unbind(overrides, id)),
      reset: (id) => commit(resetOne(overrides, id, isMac)),
      resetAll: () => commit({}),
      // A saved key whose action this build no longer has. Shown, not dropped,
      // so the settings page can say so and clear it.
      staleBindings: staleOverrides(overrides),
      clearStale: () => commit(clearStaleOverrides(overrides)),
    }),
    [bindings, browserKeys, overrides, run, available, source, register, commit],
  )

  return <KeyboardContext.Provider value={value}>{children}</KeyboardContext.Provider>
}

export function useKeyboard() {
  return useContext(KeyboardContext)
}

function useRegistration(kind, id, fn, enabled) {
  const ctx = useContext(KeyboardContext)
  const fnRef = useRef(fn)
  fnRef.current = fn
  const enabledRef = useRef(enabled)
  enabledRef.current = enabled
  const registerFn = ctx?.[kind]
  // The register functions are rebuilt with the bindings, but a handler's
  // registration only needs to exist - the refs carry the live closure.
  const stable = useRef(registerFn)
  stable.current = registerFn
  useEffect(() => {
    if (isMobile || !stable.current) return
    return stable.current(id, { fn: fnRef, enabled: enabledRef })
  }, [id])
}

// Handle an action while `enabled`. Return false from the handler when it
// can't act right now, so the key falls through.
export function useShortcut(id, fn, enabled = true) {
  useRegistration('registerHandler', id, fn, enabled)
}

// Offer the palette a list to pick from (models, efforts, permission
// words) while `enabled`: `get` returns { title, items, selectedId,
// choose } or null.
export function usePaletteSource(kind, get, enabled = true) {
  useRegistration('registerSource', kind, get, enabled)
}

// "Title (Ctrl+N)" when the action has a key, the plain title when not.
export function useHint(id) {
  const ctx = useContext(KeyboardContext)
  const label = ctx && !isMobile ? ctx.label(id) : ''
  return (title) => (label ? `${title} (${label})` : title)
}

// Ctrl+Alt+1..9: the Nth model of the focused composer's model list.
// `pick(i)` returns false when there is no Nth model (or no switching).
export function useModelSwitches(pick, enabled = true) {
  useShortcut('model-1', () => pick(0), enabled)
  useShortcut('model-2', () => pick(1), enabled)
  useShortcut('model-3', () => pick(2), enabled)
  useShortcut('model-4', () => pick(3), enabled)
  useShortcut('model-5', () => pick(4), enabled)
  useShortcut('model-6', () => pick(5), enabled)
  useShortcut('model-7', () => pick(6), enabled)
  useShortcut('model-8', () => pick(7), enabled)
  useShortcut('model-9', () => pick(8), enabled)
}

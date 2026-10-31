import { useEffect, useRef, useState } from 'react'
import Sidebar from './Sidebar.jsx'
import TitleBar from './TitleBar.jsx'
import NewChat from './NewChat.jsx'
import ChatView from './ChatView.jsx'
import CloudMode from './CloudMode.jsx'
import Onboarding from './Onboarding.jsx'
import DaemonStatus from './DaemonStatus.jsx'
import SettingsView from './SettingsView.jsx'
import PluginsView from './PluginsView.jsx'
import AutomationsView from './AutomationsView.jsx'
import MachinesView from './MachinesView.jsx'
import AccountView from './AccountView.jsx'
import MobileView from './MobileView.jsx'
import MetricsView from './MetricsView.jsx'
import BrowserPane from './BrowserPane.jsx'
import { onBrowserRequest, openedKey, shouldAutoOpen } from './browser-placement.js'
import { DEFAULT_PANE_WIDTH, MIN_PANE_WIDTH, clampPaneWidth, loadPaneWidth, savePaneWidth } from './pane-width.js'
import FloatingBar from './FloatingBar.jsx'
import { ConsoleProvider, useConsole } from './state/console.jsx'
import { needsSignIn, sessionsForMode } from './cloud.js'
import CloudNotices from './CloudNotices.jsx'
import { hasTauri, isMobile } from './platform.js'
import { connectionNotice } from './connection-notice.js'
import { keepNav, pruneNav } from './nav.js'
import { paneChatId } from './pane-header.js'
import { hashWriteReady } from './settings.js'
import { Archive, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Copy, X } from '@phosphor-icons/react'
import { pushBack, useBack } from './use-back.js'
import SplitArea from './SplitArea.jsx'
import ViewBoundary from './ViewBoundary.jsx'
import ContextMenu from './ContextMenu.jsx'
import { textFieldOf, textMenuItems } from './text-menu.js'
import CommandPalette from './CommandPalette.jsx'
import ShortcutSheet from './ShortcutSheet.jsx'
import ChangesModal from './ChangesModal.jsx'
import RaceModal from './RaceModal.jsx'
import SaveLayoutModal from './SaveLayoutModal.jsx'
import RestoreLayoutConfirm from './RestoreLayoutConfirm.jsx'
import { fitLayout, restoreConfirm } from './layouts.js'
import RenameChatModal from './RenameChatModal.jsx'
import { overlayStale } from './rename-chat.js'
import { canSeedChat, chatSetup } from './chat-setup.js'
import { pickable } from './harness-manager.js'
import { chatTitle } from './shape.js'
import ViewPopup from './ViewPopup.jsx'
import { POPUP_VIEWS, onPopup } from './popups.js'
import { KeyboardProvider, useKeyboard, useShortcut } from './keyboard.jsx'
import { freePanes, raceLayout } from './race.js'
import {
  MAX_PANES,
  closeLeaf,
  focusByIndex,
  focusLeaf,
  leafById,
  leaves,
  moveLeaf,
  newLayout,
  paneCount,
  reconcilePanes,
  restoreLayout,
  secondaryChatIds,
  setLeafView,
  splitLeaf,
} from './split.js'


const OB_KEY = 'openremote-onboarded'
const STORE_KEY = 'openremote-view-state'
const MODE_KEY = 'openremote-mode'
const SPLIT_KEY = 'openremote-split-layout'
const STATIC_VIEWS = ['new', 'plugins', 'automations', 'machines', 'settings', 'account', 'mobile', 'metrics']

// The mode the user left the app in. Local unless they chose Cloud, so a
// first launch never asks anyone to sign in. A phone runs no chats of its
// own, so it is always in Cloud.
function loadMode() {
  if (isMobile) return 'cloud'
  try {
    return localStorage.getItem(MODE_KEY) === 'cloud' ? 'cloud' : 'local'
  } catch {
    return 'local'
  }
}

const EMPTY_GONE = new Set()

function loadViewState(sessions, gone = EMPTY_GONE) {
  let saved = null
  try {
    saved = JSON.parse(sessionStorage.getItem(STORE_KEY) ?? 'null')
  } catch {
    saved = null
  }
  const known = new Set([...STATIC_VIEWS, ...(sessions ?? []).map((s) => s.id)])
  const hashView = location.hash.replace(/^#/, '')
  const fromHash = known.has(hashView) ? hashView : null
  // The stored history is kept as it stands, minus the chats this console
  // knows are gone (`gone` comes from the same rule the chat records use).
  // It used to be filtered by the current beat, so any beat that happened
  // to come back short - the first one after a reconnect is empty - took
  // the user's back stack with it.
  const stored = Array.isArray(saved?.history)
    ? saved.history.filter((view) => typeof view === 'string' && view.length > 0)
    : []
  const pruned = stored.length
    ? pruneNav(
        { history: stored, hIndex: Math.min(saved.hIndex ?? 0, stored.length - 1) },
        gone,
      )
    : { history: [], hIndex: 0 }
  let history = pruned.history
  let hIndex = pruned.hIndex
  if (history.length === 0) {
    history = ['new']
    hIndex = 0
  }
  if (fromHash && history[hIndex] !== fromHash) {
    history = [...history.slice(0, hIndex + 1), fromHash]
    hIndex = history.length - 1
  }
  return { history, hIndex }
}

function loadLayout() {
  // A phone has no panes: what a wide window stored is not a layout a phone
  // can use, so the stored one is not restored there at all.
  if (isMobile) return newLayout()
  try {
    // A pane can hold a chat or a New chat composer, never a static page, so
    // one restored from an older build is repaired before it is drawn.
    return reconcilePanes(
      restoreLayout(JSON.parse(sessionStorage.getItem(SPLIT_KEY) ?? 'null')),
      new Set(STATIC_VIEWS),
    )
  } catch {
    return newLayout()
  }
}

const isChatView = (v) => !STATIC_VIEWS.includes(v)

function Shell() {
  const {
    sessions,
    chats,
    connection,
    ensureChat,
    cloud,
    watchChats,
    setChatArchived,
    retryDaemon,
    renameChat,
    capabilities,
    projects,
    thisDevice,
  } = useConsole()
  // The sidebar's docked-vs-overlay shape is the viewport's, not a mount-time
  // guess: crossing 640px live swaps the layout (the alternative - reading
  // innerWidth once at mount - left the sidebar overlaid after narrowing).
  const [sidebarOpen, setSidebarOpen] = useState(() =>
    typeof window === 'undefined' ? true : window.matchMedia('(min-width: 640px)').matches,
  )
  useEffect(() => {
    if (typeof window === 'undefined') return
    const mq = window.matchMedia('(min-width: 640px)')
    const onChange = (e) => setSidebarOpen(e.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])
  // The startup preference plants the last view in the hash before first
  // paint (restoreBootPrefs in settings.js). A static seed validates
  // right here, but a chat id cannot - the session list is empty until
  // the first poll beat - so the seed is held as state until a beat
  // answers or the user navigates. The [sessions] reconcile below
  // consumes the seeded hash once a beat lands, and the hash-write
  // effect holds off until then (hashWriteReady). Null when the boot
  // hash is empty or static: nothing needs holding.
  const [bootSeed, setBootSeed] = useState(() => {
    const bootHash = location.hash.replace(/^#/, '')
    return bootHash && !STATIC_VIEWS.includes(bootHash) ? bootHash : null
  })
  // The sessions list the shell mounted with (the console's initial
  // empty one): any later identity is a beat's answer, and that answer
  // is the hold's release.
  const mountSessionsRef = useRef(sessions)
  // Chat ids a beat has carried at least once, for the navigation prune: an
  // id that was never listed is not a deleted chat, it is a chat this beat
  // has not reached yet.
  const seenIdsRef = useRef(new Set())
  const [sessionsAnswered, setSessionsAnswered] = useState(false)
  // Set from the waiting screen's "connect by address" link, for someone
  // who runs the daemon themselves: the first-run wizard opens straight on
  // its manual form.
  const [manualConnect, setManualConnect] = useState(false)
  const [{ history, hIndex }, setNav] = useState(() => loadViewState([]))
  // Split panes: more chats beside the primary view, Local and Cloud alike.
  // The primary pane follows the history above; the others hold their own.
  const [layout, setLayout] = useState(loadLayout)
  useEffect(() => {
    try {
      sessionStorage.setItem(SPLIT_KEY, JSON.stringify(layout))
    } catch {
      /* storage unavailable */
    }
  }, [layout])
  // The tour sets up this computer's harnesses; a phone has none, and
  // Cloud's sign-in screen is its welcome.
  const [onboarded, setOnboarded] = useState(() => {
    if (isMobile) return true
    try {
      return localStorage.getItem(OB_KEY) === '1'
    } catch {
      return true
    }
  })
  // Settings is a view like the others - the gear navigates to it.
  // Local vs Cloud: Local is this computer's chats; Cloud is the synced
  // chats of every device, behind a moreweb sign-in the first time.
  const [mode, setMode] = useState(loadMode)
  // The sidebar's + : opens New chat with the Add project flow engaged -
  // the same picker the home screen's own button opens.
  const [addProjectOpen, setAddProjectOpen] = useState(false)
  // The browser as a right side-pane: beside the chat instead of a whole
  // window. The tabs live in the browser store, so the pane opens and
  // closes without losing them.
  const [browserOpen, setBrowserOpen] = useState(false)
  const toggleBrowser = () => setBrowserOpen((v) => !v)
  // A chat asking for the browser, and this shell answering: the panes are the shell's,
  // so a chat (or its agent's tab) never needs to know the layout. The last request is
  // kept so the pane can be told which tab or address to show.
  const browserRequest = useRef(null)
  const [browserAsk, setBrowserAsk] = useState(null)
  // The pane's width: remembered, clamped to this window, and dragged by the grip the
  // row draws beside it.
  const [browserWidth, setBrowserWidth] = useState(loadPaneWidth)
  const startBrowserDrag = (e) => {
    e.preventDefault()
    const startX = e.clientX
    const startWidth = browserWidth
    const target = e.currentTarget
    target.setPointerCapture(e.pointerId)
    let latest = startWidth
    const move = (ev) => {
      latest = clampPaneWidth(startWidth + (startX - ev.clientX), window.innerWidth)
      setBrowserWidth(latest)
    }
    const done = () => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', done)
      target.removeEventListener('pointercancel', done)
      savePaneWidth(latest)
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', done)
    target.addEventListener('pointercancel', done)
  }
  useEffect(
    () =>
      onBrowserRequest((request) => {
        browserRequest.current = request
        setBrowserAsk(request)
        setBrowserOpen(true)
      }),
    [],
  )
  // The chat whose name is being changed. Renaming lives here, not in a view,
  // because two surfaces open the same dialog: the sidebar's row menu and the
  // command palette's row. `renaming` is the record the dialog opened on;
  // `renamed` is the one-beat overlay of names just saved (the provider
  // returns the daemon's answer but does not fold it, so without it a renamed
  // row would keep its old name until the next poll).
  const [renaming, setRenaming] = useState(null)
  const [renamed, setRenamed] = useState({})

  // Each mode has its own views; one restored from history that belongs to
  // the other mode opens New chat instead. Plugins and automations act on
  // this computer, so the desktop keeps them in Cloud too; a phone runs no
  // chats, so it has neither.
  const OTHER_MODE = { local: ['machines'], cloud: isMobile ? ['plugins', 'automations'] : [] }
  const stored = history[hIndex]
  // Settings, Metrics and Mobile open over the app (popups.js), never in
  // a pane; one left in the history from before opens New chat. The
  // browser used to be a whole-window view; one left in the history opens
  // New chat, and its tabs are still there when the side pane opens.
  //
  // A chat belongs to a mode: a private chat is this computer's, a synced one is
  // the account's. Switching mode with panes open used to keep the other mode's
  // chat in the primary pane, so a private chat went on streaming under a window
  // that said Cloud. The mode's own list rule decides (cloud.js), and a chat it
  // does not hold opens New chat.
  const chatInMode = (id) =>
    !STATIC_VIEWS.includes(id) &&
    sessionsForMode(sessions, mode, thisDevice).some((s) => s.id === id)
  // A chat just created is in the console before the session list catches
  // up. Treating that gap as "the other mode" bounced the view back to New
  // chat, so sending never landed in the chat it had just made.
  const listed = stored != null && (sessions ?? []).some((s) => s.id === stored)
  const view =
    OTHER_MODE[mode].includes(stored) ||
    POPUP_VIEWS.includes(stored) ||
    stored === 'browser' ||
    (listed && chats[stored] && !chatInMode(stored))
      ? 'new'
      : stored
  const chat = STATIC_VIEWS.includes(view) ? null : chats[view] ?? null

  // Whether this window has had a working connection at any point, which is
  // the one fact that tells a daemon that stopped apart from one that never
  // answered. Set once and never cleared: the row's words are about this
  // session, not about the last beat.
  const [everConnected, setEverConnected] = useState(false)
  useEffect(() => {
    if (connection.state === 'connected') setEverConnected(true)
  }, [connection.state])

  // The daemon row above the main area, or nothing. A view that prints the
  // same news in its own error line (a chat, the New chat home) keeps the
  // row away; the words themselves live in connection-notice.js.
  const daemonRow = connectionNotice({
    state: connection.state,
    everConnected,
    restartable: hasTauri && !isMobile,
    showsOwnError: view === 'new' || isChatView(view),
    error: connection.error,
  })

  // Every chat on screen streams live - the primary's and each pane's -
  // but only one that still exists. A chat deleted on another device
  // loses its record on the poll beat that drops it, so it leaves the
  // watched set here (its pane shows the fallback) and no stream keeps
  // asking the daemon for a session it no longer has.
  const watchKey = [
    ...(isChatView(view) && chats[view] ? [view] : []),
    ...secondaryChatIds(layout, isChatView).filter((id) => chats[id]),
  ].join(',')
  useEffect(() => {
    watchChats(watchKey ? watchKey.split(',') : [])
  }, [watchKey, watchChats])

  // Reconcile navigation with the live session list (a chat in the history
  // that no longer exists falls back to New chat). A chat reached by hash
  // still needs its record - ensure it, the same as a sidebar open.
  useEffect(() => {
    // The first beat's answer (a new sessions identity - the mount-time
    // list is the console's initial empty one, not an answer) marks the
    // session list as loaded. This run reads the still-seeded hash
    // against the real list, which is the seed's consumption; from here
    // the hash-write effect may run again (hashWriteReady).
    if (sessions !== mountSessionsRef.current) setSessionsAnswered(true)
    // The chats a beat has listed at least once, and the ones whose record
    // the console dropped: a history step goes only when its chat was
    // listed before, is listed no longer, and has no record left - the same
    // rule `vanishedChatIds` uses in the state layer. A step whose chat was
    // never in a beat (a reconnect's empty first beat, a chat created
    // between beats) is not gone, and the back stack keeps it.
    const seen = seenIdsRef.current
    const live = new Set((sessions ?? []).map((s) => s.id))
    for (const id of live) seen.add(id)
    const gone = new Set()
    for (const id of seen) {
      if (!live.has(id) && !chats[id]) gone.add(id)
    }
    const state = loadViewState(sessions, gone)
    // A beat rebuilds the view state from storage whether or not anything
    // about it changed. Storing it only when it actually differs keeps the
    // shell from re-rendering on the clock, mid-stream.
    setNav((previous) => keepNav(previous, state))
    // The same gone set repairs the panes: a pane whose chat was deleted in
    // this window or another one becomes a New chat instead of pointing at
    // nothing for the rest of the session. reconcilePanes keeps the object
    // when there is nothing to repair, so an ordinary beat does not re-render.
    setLayout((current) => reconcilePanes(current, gone))
    const view = state.history[state.hIndex]
    const session = (sessions ?? []).find((s) => s.id === view)
    if (session) ensureChat(session)
    // Panes restored from the last visit need their records too.
    for (const id of secondaryChatIds(layout, isChatView)) {
      const s = (sessions ?? []).find((x) => x.id === id)
      if (s) ensureChat(s)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessions])

  useEffect(() => {
    try {
      sessionStorage.setItem(STORE_KEY, JSON.stringify({ history, hIndex }))
    } catch {
      /* storage unavailable */
    }
  }, [history, hIndex])

  useEffect(() => {
    // This effect runs on mount, and its replaceState would clobber a
    // boot-seeded chat id to #new before any session beat arrived to
    // validate it (the [sessions] reconcile above is the hash's only
    // reader), stamping 'new' over the last-view record too. So it
    // holds until the seed is resolved: a beat answered, or no seed is
    // pending (never planted, or the user navigated). The rule lives in
    // hashWriteReady so it is tested.
    if (!hashWriteReady({ sessionsLoaded: sessionsAnswered, seedConsumed: bootSeed === null }))
      return
    // Keep the entry's state: on a phone it is a back step.
    window.history.replaceState(window.history.state, '', `#${view}`)
    // The last view the user was on, for the startup preference's
    // restore path. Written only once onboarding is done - the tour's
    // own views are not the user's last view.
    if (onboarded) {
      try {
        localStorage.setItem('openremote-last-view', view)
      } catch {
        /* storage unavailable */
      }
    }
  }, [view, onboarded, sessionsAnswered, bootSeed])

  const navigate = (id) => {
    // The user's click supersedes the boot seed - release the hash hold
    // so the write effect reflects where they chose to go. Even a click
    // landing on the current view is their intent.
    setBootSeed(null)
    if (id === view) return
    setNav((nav) => ({
      history: [...nav.history.slice(0, nav.hIndex + 1), id],
      hIndex: nav.hIndex + 1,
    }))
    // Android's back button returns to the view before.
    if (isMobile) pushBack(goBack)
  }

  const keepMode = (next) => {
    setMode(next)
    try {
      localStorage.setItem(MODE_KEY, next)
    } catch {
      /* storage unavailable */
    }
  }

  const switchMode = (next) => {
    keepMode(next)
    // With panes open the layout is the user's own; the primary view only
    // falls back when it belongs to the other mode (see `view` above).
    if (paneCount(layout) === 1) navigate('new')
  }

  function goBack() {
    // Back and forward are the user's navigation too - the seed gives way.
    setBootSeed(null)
    setNav((nav) => ({ ...nav, hIndex: Math.max(0, nav.hIndex - 1) }))
  }
  const goForward = () => {
    setBootSeed(null)
    setNav((nav) => ({ ...nav, hIndex: Math.min(nav.history.length - 1, nav.hIndex + 1) }))
  }

  // Opening a chat from the sidebar: make sure its record exists even
  // before the next session poll.
  const openSession = (id) => {
    // Opening a chat leaves New chat, so a setup seed from an earlier press is
    // spent: a later New chat must open on its own pickers, not on that seed.
    setSetupSeed(null)
    const session = (sessions ?? []).find((s) => s.id === id)
    if (session) ensureChat(session)
    navigate(id)
    if (typeof window !== 'undefined' && !window.matchMedia('(min-width: 640px)').matches)
      setSidebarOpen(false)
  }

  // The sidebar's click goes where the user is looking: the focused pane
  // when that is a split pane and the target is a chat or New chat; every
  // other view (plugins, settings, ...) belongs to the primary pane.
  const focused = leafById(layout, layout.focus)
  // The chat the focused pane is showing, when it is showing one. The palette's
  // rename row is about "this chat", and the address cannot answer that alone: a
  // split can have its focus elsewhere, and the primary pane's view is null by
  // design, so the rule for "which chat is this pane about" lives in one place.
  const focusedChatId = paneChatId(focused?.view, isChatView(view) ? view : null)
  const openInFocus = (id) => {
    // The same rule for the focused pane's own route: an ordinary New chat
    // (the tile, the shortcut) is not a press of the setup row.
    if (id === 'new') setSetupSeed(null)
    if (focused?.view !== null && (id === 'new' || isChatView(id))) {
      const session = (sessions ?? []).find((s) => s.id === id)
      if (session) ensureChat(session)
      setLayout((l) => setLeafView(l, focused.id, id))
      return
    }
    setLayout((l) => focusLeaf(l, leaves(l.root).find((p) => p.view === null).id))
    openSession(id)
  }

  // Open the rename dialog on one chat: the sidebar passes its row's id, and a
  // palette row passes the focused pane's, which is the default here so the
  // palette can call it with no argument. The title comes from the console's
  // record so the field starts where the user expects.
  const openRename = (id = focusedChatId) => {
    if (!id) return
    const record = chats[id]
    const session = (sessions ?? []).find((s) => s.id === id)
    setRenaming({
      id,
      title: chatTitle({ title: record?.title ?? session?.title }),
      synced: Boolean(record?.executor ?? session?.executor),
    })
  }

  // The console call the slash command makes, with one beat of optimism: the
  // list shows the new name at once, and a failure takes it back before the
  // dialog hears about it, so the dialog stays open with the reason. The
  // overlay carries the stamp the daemon answered with, because a name alone
  // cannot tell an echo from a later rename made on another device.
  const renameTo = async (id, next) => {
    setRenamed((map) => ({ ...map, [id]: { name: next, stamp: null } }))
    try {
      const saved = await renameChat(id, next)
      setRenamed((map) => ({ ...map, [id]: { name: next, stamp: saved?.updated_at ?? null } }))
    } catch (err) {
      setRenamed((map) => {
        const left = { ...map }
        delete left[id]
        return left
      })
      throw err
    }
  }

  // The overlay has done its job once the console's own record carries the
  // name, and it has to go then: kept longer, it would hide a later rename
  // made on another device. "Carries it" is a stamp comparison, not a string
  // comparison, because two devices can hand the same name to two different
  // records and the newer one wins.
  useEffect(() => {
    if (Object.keys(renamed).length === 0) return
    setRenamed((map) => {
      const left = {}
      let stale = false
      for (const [id, entry] of Object.entries(map)) {
        const session = (sessions ?? []).find((s) => s.id === id)
        const known = chats[id]?.updated_at ?? session?.updated_at
        if (overlayStale(entry?.stamp ?? null, known)) stale = true
        else left[id] = entry
      }
      return stale ? left : map
    })
  }, [chats, sessions, renamed])

  // Open a chat in a new pane beside the focused one.
  const openBeside = (id, dir = 'row', before = false) => {
    const session = (sessions ?? []).find((s) => s.id === id)
    if (session) ensureChat(session)
    const next = splitLeaf(layout, layout.focus, dir, id, before)
    if (next === layout) openInFocus(id)
    else setLayout(next)
  }

  // The race's chats arrived: one pane each, in the order they were picked.
  // The plan lives in race.js, and it hands back the chat the primary pane must
  // navigate to (an empty primary's view is the navigation, not a leaf field)
  // plus the panes to open beside it, so the shell keeps owning the layout.
  const startRace = (ids) => {
    const { primary, layout: next } = raceLayout(layout, ids)
    if (primary) openSession(primary)
    if (next !== layout) setLayout(next)
  }

  // Whether a drop on a pane edge may open a new pane at all, read by the
  // drop and by the pane that draws the tint. The cap and the phone rule live
  // here once: splitting is refused at four panes, and a phone has no panes.
  const panesCanSplit = !isMobile && paneCount(layout) < MAX_PANES

  // A chat dropped on a pane: the middle opens it in that pane, an outer band
  // moves it into a new pane on that side. Where no split can happen the drop
  // opens in the pane it landed on, and a chat dropped back on the pane it
  // already lives in is not a move.
  const dropInPane = (leafId, id, place) => {
    const leaf = leafById(layout, leafId)
    if (!leaf || leaf.view === id) return
    const session = (sessions ?? []).find((s) => s.id === id)
    if (session) ensureChat(session)
    if (place.zone === 'center' || !panesCanSplit) {
      if (leaf.view === null) {
        openSession(id)
      } else {
        setLayout((l) => focusLeaf(setLeafView(l, leaf.id, id), leaf.id))
      }
      return
    }
    // A move, not a copy: the pane the chat came from becomes a New chat, so
    // the same chat never streams in two panes with two drafts. The primary is
    // left alone (its view is navigation), and a sidebar drag has no source.
    const source = leaves(layout.root).find((pane) => pane.view === id)
    setLayout((l) => moveLeaf(l, source?.id ?? null, leafId, place.dir, place.before, id))
  }

  // Right-click in the main area: split from the pane under the cursor,
  // and act on the chat it shows. Text fields are handled by the one listener
  // below, before this sees them.
  const [paneMenu, setPaneMenu] = useState(null)
  // Text fields: the engine's own context menu is off in the console, so a
  // right click in an input, a textarea or a contenteditable had nothing at
  // all. One capture listener at the document serves every field in every
  // view and popup, and it only acts on fields: the pane menu and the
  // sidebar's row menu are left exactly as they were.
  const [textMenu, setTextMenu] = useState(null)
  useEffect(() => {
    const onMenu = (e) => {
      const el = textFieldOf(e.target)
      if (!el) return
      e.preventDefault()
      e.stopPropagation()
      setTextMenu({ x: e.clientX, y: e.clientY, el })
    }
    document.addEventListener('contextmenu', onMenu, true)
    return () => document.removeEventListener('contextmenu', onMenu, true)
  }, [])
  const onMainContextMenu = (e) => {
    if (isMobile) return
    const t = e.target
    if (t.closest?.('input, textarea, select, a, [contenteditable="true"], .ctx-menu, .nc-menu')) return
    if (window.getSelection()?.toString()) return
    const leafId = t.closest?.('[data-leaf-id]')?.dataset.leafId ?? layout.focus
    e.preventDefault()
    setLayout((l) => focusLeaf(l, leafId))
    setPaneMenu({ x: e.clientX, y: e.clientY, leafId })
  }

  const archiveFrom = (leaf, id) => {
    setChatArchived(id, true)
    if (leaf.view === null) navigate('new')
    else setLayout((l) => setLeafView(l, leaf.id, 'new'))
  }

  const paneMenuItems = (leafId) => {
    const leaf = leafById(layout, leafId)
    if (!leaf) return []
    const v = leaf.view ?? view
    const full = paneCount(layout) >= MAX_PANES
    const split = (dir, before) => () => setLayout((l) => splitLeaf(l, leafId, dir, 'new', before))
    // The same four words as the pane header's menu, so a split is called
    // one thing everywhere it can be started.
    const items = [
      ...(full ? [{ header: `${MAX_PANES} panes is the most` }] : []),
      { label: 'Split right', icon: <ArrowRight size={14} />, hint: kb.label('split-right'), disabled: full, onSelect: split('row', false) },
      { label: 'Split down', icon: <ArrowDown size={14} />, hint: kb.label('split-down'), disabled: full, onSelect: split('col', false) },
      { label: 'Split left', icon: <ArrowLeft size={14} />, disabled: full, onSelect: split('row', true) },
      { label: 'Split up', icon: <ArrowUp size={14} />, disabled: full, onSelect: split('col', true) },
    ]
    const title = chats[v]?.title
    if (isChatView(v) && title) {
      items.push(
        { divider: true },
        { label: 'Archive chat', icon: <Archive size={14} />, hint: kb.label('archive'), onSelect: () => archiveFrom(leaf, v) },
        {
          label: 'Copy chat title',
          icon: <Copy size={14} />,
          onSelect: () => navigator.clipboard?.writeText(title).catch(() => {}),
        },
      )
    }
    if (leaf.view !== null) {
      items.push(
        { divider: true },
        { label: 'Close pane', icon: <X size={14} />, hint: kb.label('close-pane'), onSelect: () => setLayout((l) => closeLeaf(l, leafId)) },
      )
    }
    return items
  }

  // The sidebar's + : New chat, with the Add project flow engaged.
  const addProject = () => {
    setAddProjectOpen(true)
    navigate('new')
    if (typeof window !== 'undefined' && !window.matchMedia('(min-width: 640px)').matches)
      setSidebarOpen(false)
  }

  const finishOnboarding = () => {
    try {
      localStorage.setItem(OB_KEY, '1')
    } catch {
      /* storage unavailable */
    }
    setOnboarded(true)
  }

  const replayOnboarding = () => {
    try {
      localStorage.removeItem(OB_KEY)
    } catch {
      /* storage unavailable */
    }
    setOnboarded(false)
  }

  // On a phone the sidebar is a drawer over the page; back closes it.
  const [narrow, setNarrow] = useState(
    () => typeof window !== 'undefined' && !window.matchMedia('(min-width: 640px)').matches,
  )
  useEffect(() => {
    if (typeof window === 'undefined') return
    const mq = window.matchMedia('(min-width: 640px)')
    const onChange = (e) => setNarrow(!e.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])
  useBack(narrow && sidebarOpen, () => setSidebarOpen(false))

  // Account's devices row crosses to Machines, which is Cloud's view -
  // the crossover switches the mode too, so the button never lands dead.
  const openMachines = () => {
    if (mode !== 'cloud') switchMode('cloud')
    navigate('machines')
  }

  // Keyboard actions the shell owns (keyboard.jsx): navigation, views,
  // the focused chat's archive, and the layout. A view that belongs to
  // the other mode switches the mode on the way, like Account's
  // crossover to Machines.
  const kb = useKeyboard()
  const desk = onboarded && !isMobile
  const [palette, setPalette] = useState(false)
  // The race modal: the palette's row opens it, and it hands back the chats it
  // created so the shell can lay them out in the panes it already owns.
  const [racing, setRacing] = useState(false)
  // Saving the window's shape: the palette's row opens the dialog, and the
  // dialog writes it through layouts.js into this console's own storage.
  const [savingLayout, setSavingLayout] = useState(false)
  // A saved shape that would replace panes already open: asked once, then applied.
  const [pendingRestore, setPendingRestore] = useState(null)
  // The view open over the app, if any: settings, metrics or mobile.
  const [popup, setPopup] = useState(null)
  useEffect(() => onPopup((name) => setPopup(name)), [])
  const [sheet, setSheet] = useState(false)
  const [presetProject, setPresetProject] = useState(null)
  // A New chat opened from "New chat with this setup": the fields chat-setup.js
  // read off the chat, plus a nonce so a second press of the same row applies
  // again (the view settles it once per nonce).
  const [setupSeed, setSetupSeed] = useState(null)
  const [openDevice, setOpenDevice] = useState(null)
  const goView = (id) => {
    if (POPUP_VIEWS.includes(id)) {
      setPopup(id)
      return
    }
    // Machines is Cloud's; plugins and automations live in both modes.
    const needs = { machines: 'cloud' }[id]
    if (needs && mode !== needs) keepMode(needs)
    openInFocus(id)
  }

  // A saved shape arrived from the palette's Layouts page or the pane header,
  // already checked against this window's room and already resolved (a chat role
  // is the chat current now). A restore replaces whatever panes are open, so it
  // asks once when something would be lost, then applies the shape with every
  // pane held above the readable floor. Setting the tree is the restore; the
  // primary pane's view is navigation, so that is the one thing the shell still
  // has to do.
  const restoreNow = (plan) => {
    // Fractions carry no pixel floor of their own: a shape saved in a wide
    // window restored into a narrow one came back as slivers. innerWidth is the
    // whole window rather than the pane area, so the floor is slightly weaker
    // than the panes deserve, and it is the only width the shell knows here.
    setLayout(fitLayout(plan.layout, { width: typeof window === 'undefined' ? 0 : window.innerWidth }))
    if (!plan.primaryRole) return
    if (plan.primaryRole === 'new') navigate('new')
    else if (isChatView(plan.primaryRole)) openSession(plan.primaryRole)
    else goView(plan.primaryRole)
  }
  const applySavedLayout = (plan) => {
    if (!plan?.ok) return
    const ask = restoreConfirm(plan, layout)
    if (ask) return setPendingRestore({ plan, ask })
    restoreNow(plan)
  }
  // A new chat already in a project (the sidebar's + on a project, the
  // palette's project rows): the home screen with that project chosen and
  // its message box focused.
  const newChatIn = (id) => {
    // A project row opens a New chat of its own chosen project, so any setup
    // left from the last press of "New chat with this setup" is spent.
    setSetupSeed(null)
    setPresetProject({ id, nonce: Date.now() })
    focusPrimary()
    navigate('new')
    if (typeof window !== 'undefined' && !window.matchMedia('(min-width: 640px)').matches)
      setSidebarOpen(false)
  }
  // "New chat with this setup": the focused chat's own fields, read by the same
  // rule the palette offers the row on, and handed to New chat as a seed it
  // settles against today's lists (the preset path in NewChat.jsx). Without a
  // harness this console can offer there is no setup to copy, so nothing opens:
  // a New chat on a different harness is not the setup that was asked for.
  const newChatWithSetup = (chatId) => {
    const record = chats?.[chatId] ?? (sessions ?? []).find((s) => s.id === chatId) ?? null
    const setup = chatSetup(record, {
      harnesses: (capabilities?.harnesses ?? []).filter(pickable),
      projects: projects ?? [],
      thisDevice,
    })
    if (!canSeedChat(setup)) return
    setSetupSeed({ nonce: Date.now(), ...setup })
    focusPrimary()
    navigate('new')
    if (typeof window !== 'undefined' && !window.matchMedia('(min-width: 640px)').matches)
      setSidebarOpen(false)
  }
  const focusPrimary = () =>
    setLayout((l) => focusLeaf(l, leaves(l.root).find((p) => p.view === null).id))
  useShortcut('new-chat', () => openInFocus('new'), desk)
  useShortcut('palette', () => setPalette(true), desk)
  useShortcut('cheat-sheet', () => setSheet(true), desk)
  useShortcut('view-settings', () => goView('settings'), desk)
  useShortcut('view-automations', () => goView('automations'), desk)
  useShortcut('view-plugins', () => goView('plugins'), desk)
  useShortcut('view-machines', () => goView('machines'), desk)
  useShortcut('view-account', () => goView('account'), desk)
  useShortcut('view-mobile', () => goView('mobile'), desk)
  useShortcut('view-metrics', () => goView('metrics'), desk)
  useShortcut(
    'archive',
    () => {
      const v = focused?.view ?? view
      if (!focused || !isChatView(v) || !chats[v]) return false
      archiveFrom(focused, v)
    },
    desk,
  )
  useShortcut('toggle-sidebar', () => setSidebarOpen((v) => !v), desk)
  useShortcut('toggle-browser', toggleBrowser, desk)
  // The agent of the chat in the primary pane opening a tab opens the browser beside it,
  // showing that tab. Another chat's agent never steals the view: its tab carries its
  // badge in the browser instead. A pane the person closed stays closed until the agent
  // opens another tab, which is what `opened` remembers.
  const primaryChat = useRef(null)
  primaryChat.current = isChatView(view) ? view : null
  useEffect(() => {
    if (!hasTauri || isMobile) return undefined
    const opened = new Set()
    let stop = null
    let gone = false
    import('@tauri-apps/api/event')
      .then(({ listen }) =>
        listen('browser:tab', ({ payload }) => {
          if (!shouldAutoOpen(payload, primaryChat.current, opened)) return
          opened.add(openedKey(payload))
          browserRequest.current?.({ chatId: payload.controller.chat, tab: payload.id, focus: false })
        }),
      )
      .then((unlisten) => {
        if (gone) unlisten()
        else stop = unlisten
      })
      .catch(() => {})
    return () => {
      gone = true
      stop?.()
    }
  }, [])
  useShortcut('toggle-mode', () => switchMode(mode === 'local' ? 'cloud' : 'local'), desk)
  useShortcut('split-right', () => setLayout((l) => splitLeaf(l, l.focus, 'row')), desk)
  useShortcut('split-down', () => setLayout((l) => splitLeaf(l, l.focus, 'col')), desk)
  // Ctrl+1..4: the panes in reading order, the way VS Code numbers its
  // editor groups.
  useShortcut('focus-pane-1', () => setLayout((l) => focusByIndex(l, 0)), desk)
  useShortcut('focus-pane-2', () => setLayout((l) => focusByIndex(l, 1)), desk)
  useShortcut('focus-pane-3', () => setLayout((l) => focusByIndex(l, 2)), desk)
  useShortcut('focus-pane-4', () => setLayout((l) => focusByIndex(l, 3)), desk)
  // The primary pane is the window's own view; only a split pane closes.
  useShortcut(
    'close-pane',
    () => {
      if (!focused || focused.view === null) return false
      setLayout((l) => closeLeaf(l, focused.id))
    },
    desk,
  )
  // The palette's recent chats: the navigation history, newest first.
  const recentChatIds = [...new Set([...history].reverse().filter(isChatView))]
  const primaryFocused = focused?.view === null

  // What the primary pane shows: the view the app's navigation points at.
  // Until this device is in an account, home is the sign-in door. Every
  // other view still opens - a QR connection does not sign in, and
  // Machines, chats, and the rest carry their own empty state.
  const primaryContent = (
    view === 'settings' ? (
    <SettingsView
      onReplay={replayOnboarding}
      onOpenChat={openSession}
      // The Daemon section gets a row that opens the manual connect form: the
      // connection errors say "reconnect from Settings", and Settings has to
      // be able to do it.
      onManualConnect={() => setManualConnect(true)}
    />
  ) : view === 'account' ? (
    <AccountView onOpenMachines={openMachines} />
  ) : view === 'mobile' ? (
    <MobileView onOpenChat={openSession} />
  ) : view === 'metrics' ? (
    <MetricsView />
  ) : view === 'plugins' ? (
    // Plugins and automations are this computer's: no Cloud sign-in needed.
    <PluginsView />
  ) : view === 'automations' ? (
    <AutomationsView onOpenChat={openSession} />
  ) : mode === 'cloud' && needsSignIn(cloud) && view === 'new' ? (
    <CloudMode onBackToLocal={isMobile ? null : () => switchMode('local')} />
  ) : (
    <>
      {view === 'new' &&
        (bootSeed && !sessionsAnswered && connection.state !== 'error' ? (
          // The boot hash names a chat the first session beat has not
          // answered for yet. The composer here would take the first sentence
          // of the day and drop it when the beat moves the view to the chat.
          //
          // It ends when the first read answers, or when the boot itself
          // failed: a daemon that never answered would otherwise leave this
          // sentence standing for good, over a daemon row that already says
          // why. Nothing else ends it - a slow beat is still a beat coming.
          <div className="cv-none">Opening your chat...</div>
        ) : (
          <NewChat
            onOpen={openSession}
            mode={mode}
            addProjectOpen={addProjectOpen}
            onAddProjectDone={() => setAddProjectOpen(false)}
            presetProject={presetProject}
            setupSeed={setupSeed}
            focused={primaryFocused}
            onOpenPlugins={() => goView('plugins')}
            topRight={<FloatingBar browserOn={browserOpen} onBrowser={toggleBrowser} />}
          />
        ))}
      {view === 'machines' && <MachinesView onOpenChat={openSession} openDevice={openDevice} />}
      {!STATIC_VIEWS.includes(view) &&
        (chat ? (
          <ChatView key={chat.id} chat={chat} focused={primaryFocused} />
        ) : (
          <div className="cv-none">That chat is no longer in this workspace.</div>
        ))}
    </>
    )
  )

  const paneLabel = (leaf) => {
    const v = leaf.view ?? view
    if (v === 'new') return 'New chat'
    if (STATIC_VIEWS.includes(v)) return v[0].toUpperCase() + v.slice(1)
    return chats[v]?.title ?? 'Chat'
  }

  // A split pane holds a chat or a New chat composer; the same chats and
  // composers serve Local and Cloud, so one window mixes both.
  const paneContent = (leaf) => {
    if (leaf.view === null) return primaryContent
    if (leaf.view === 'new') {
      return (
        <NewChat
          onOpen={(id) => setLayout((l) => setLeafView(l, leaf.id, id))}
          mode={mode}
          focused={leaf.id === layout.focus}
        />
      )
    }
    const paneChat = chats[leaf.view]
    return paneChat ? (
      <ChatView key={paneChat.id} chat={paneChat} focused={leaf.id === layout.focus} />
    ) : (
      <div className="cv-none">That chat is no longer in this workspace.</div>
    )
  }

  // Every pane's view is drawn behind a boundary, so a view that throws
  // takes only itself down: the pane answers with a panel that says what
  // happened, while the sidebar, the titlebar and the other panes go on
  // working. The key is the view this pane shows, so navigating away and
  // back hands the next view a boundary with no failure in it; without
  // that, a panel would outlive the view it was about.
  const renderPane = (leaf) => (
    <ViewBoundary
      key={leaf.view ?? view}
      onNewChat={() => {
        if (leaf.view === null) {
          navigate('new')
          return
        }
        setLayout((l) => focusLeaf(setLeafView(l, leaf.id, 'new'), leaf.id))
      }}
    >
      <div key={leaf.view ?? view} className="pane-switch">
        {paneContent(leaf)}
      </div>
    </ViewBoundary>
  )

  const sheetRef = useRef(null)
  const frameRef = useRef(null)
  const ignoreBackdropClick = useRef(false)

  // A left-to-right drag on the page opens the drawer. It does not have
  // to start on the screen edge - that strip is where the system back
  // gesture lives, and a finger is rarely perfectly horizontal.
  function beginDrawerDrag(event) {
    if (event.button != null && event.button !== 0) return
    const sheet = sheetRef.current
    const frame = frameRef.current
    if (!sheet || !frame) return
    const startX = event.clientX
    const startY = event.clientY
    const origin = sidebarOpen ? 264 : 0
    let dragging = false
    const move = (ev) => {
      const dx = ev.clientX - startX
      const dy = ev.clientY - startY
      if (!dragging) {
        if (Math.abs(dx) < 10 && Math.abs(dy) < 10) return
        const opening = !sidebarOpen && dx > 10 && Math.abs(dx) > Math.abs(dy)
        const closing = sidebarOpen && dx < -10 && Math.abs(dx) > Math.abs(dy)
        if (Math.abs(dy) > 24 && Math.abs(dy) > Math.abs(dx)) {
          stop()
          return
        }
        if (!opening && !closing) return
        dragging = true
        sheet.classList.add('dragging')
      }
      if (ev.cancelable) ev.preventDefault()
      const x = Math.max(0, Math.min(264, origin + (ev.clientX - startX)))
      sheet.style.transform = `translate3d(${x}px,0,0)`
    }
    const up = (ev) => {
      const dx = ev.clientX - startX
      const x = Math.max(0, Math.min(264, origin + dx))
      stop()
      if (!dragging) return
      ignoreBackdropClick.current = true
      const open = sidebarOpen ? dx > -48 : dx > 48
      const target = open ? 264 : 0
      const reduce = document.documentElement.hasAttribute('data-reduce-motion')
      sheet.classList.remove('dragging')
      frame.classList.toggle('drawer-open', open)
      // Hold the finger's position for one frame, then ease to the rest
      // point. Clearing the inline transform in the same frame skips that.
      sheet.style.transition = 'none'
      sheet.style.transform = `translate3d(${x}px,0,0)`
      requestAnimationFrame(() => {
        sheet.style.transition = reduce
          ? 'none'
          : 'transform 0.32s cubic-bezier(0.32, 0.72, 0, 1)'
        sheet.style.transform = `translate3d(${target}px,0,0)`
        const finish = (ev) => {
          if (ev && ev.propertyName !== 'transform') return
          sheet.removeEventListener('transitionend', finish)
          sheet.style.transition = ''
          sheet.style.transform = ''
        }
        if (reduce || Math.abs(target - x) < 1) finish()
        else sheet.addEventListener('transitionend', finish)
      })
      setSidebarOpen(open)
    }
    const stop = () => {
      window.removeEventListener('pointermove', move, true)
      window.removeEventListener('pointerup', up, true)
      window.removeEventListener('pointercancel', up, true)
    }
    window.addEventListener('pointermove', move, true)
    window.addEventListener('pointerup', up, true)
    window.addEventListener('pointercancel', up, true)
  }

  const sidebar = onboarded ? (
    <Sidebar
      open={sidebarOpen}
      active={popup ?? ((focused?.view ?? view) === 'new' ? null : (focused?.view ?? view))}
      mode={mode}
      onMode={switchMode}
      onSelect={(id) => (POPUP_VIEWS.includes(id) ? setPopup(id) : openInFocus(id))}
      onSplit={openBeside}
      onAddProject={addProject}
      onNewChatIn={newChatIn}
      onReveal={() => setSidebarOpen(true)}
      onBrowser={toggleBrowser}
      browserOpen={browserOpen}
      // The row menu's Rename opens the dialog hosted below; the overlay is
      // the name it shows until the console's own record catches up.
      onRenameChat={openRename}
      renamedTitles={Object.fromEntries(
        Object.entries(renamed).map(([id, entry]) => [id, entry?.name ?? entry]),
      )}
    />
  ) : null

  const body = () => (
    <div
      ref={frameRef}
      className={`app-frame${isMobile ? ' is-mobile' : ''}${isMobile && sidebarOpen ? ' drawer-open' : ''}`}
    >
      {/* On a phone the sidebar stays put. The sheet - bar and page
          together - slides right to reveal it. */}
      {isMobile && sidebar}
      <div
        className="app-sheet"
        ref={sheetRef}
        onPointerDown={isMobile && onboarded ? beginDrawerDrag : undefined}
      >
      {isMobile && sidebarOpen && (
        <div
          className="sb-backdrop"
          onClick={() => {
            if (ignoreBackdropClick.current) {
              ignoreBackdropClick.current = false
              return
            }
            setSidebarOpen(false)
          }}
        />
      )}
      {/* The frameless window has no OS chrome - the titlebar (drag,
          minimize, maximize, close) renders on every screen, onboarding
          included. */}
      <TitleBar
        sidebarOpen={sidebarOpen}
        onToggleSidebar={onboarded ? () => setSidebarOpen((v) => !v) : undefined}
        onBack={goBack}
        onForward={goForward}
        canBack={onboarded && hIndex > 0}
        canForward={onboarded && hIndex < history.length - 1}
        onSplit={onboarded ? () => setLayout((l) => splitLeaf(l, l.focus, 'row')) : undefined}
      />
      {manualConnect ? (
        <div className="app-body">
          <Onboarding
            onDone={() => {
              // Also on the way back from a daemon that stopped: a user who
              // is set up reaches the same form, and it is only a door, not
              // the first run again.
              setManualConnect(false)
              finishOnboarding()
            }}
            // The door has to close both ways: a user who opened the form to
            // look at it can come back to the screen that was healing by
            // itself, which is the only one that offers Try now.
            onDismiss={() => setManualConnect(false)}
            startManual
          />
        </div>
      ) : connection.state === 'waiting-daemon' ? (
        // No daemon at all: say so, with the shell's reason when it has
        // one, instead of leaving a setup spinner and no explanation. The
        // console keeps polling, so this leaves by itself when one starts.
        <div className="app-body">
          {/* The form is reachable whether or not this app has been set up
              before: someone whose saved address stopped answering is the
              one who needs to type a new one. */}
          <DaemonStatus onManualConnect={() => setManualConnect(true)} />
        </div>
      ) : !onboarded ? (
        <div className="app-body">
          <Onboarding onDone={finishOnboarding} />
        </div>
      ) : (
        <div className="app-body">
          {!isMobile && sidebarOpen && <div className="sb-backdrop" onClick={() => setSidebarOpen(false)} />}
          {!isMobile && sidebar}
          <main className="main" onContextMenu={onMainContextMenu}>
            {/* The daemon row comes first: while it is up, every list under
                it is the last one the daemon sent, and that is the fact the
                row is there to explain. */}
            {daemonRow && (
              <div className="cn-stack" role="status">
                <div className="cn-row">
                  <span className="cn-title">{daemonRow.title}</span>
                  <span className="cn-text">{daemonRow.note}</span>
                  {daemonRow.action === 'restart' && (
                    <button type="button" className="cn-btn primary" onClick={retryDaemon}>
                      Restart the daemon
                    </button>
                  )}
                  <button type="button" className="cn-btn" onClick={() => goView('settings')}>
                    Open Settings
                  </button>
                </div>
              </div>
            )}
            <CloudNotices />
            <div className="main-row">
              {/* One frame for the main area, split or not: with a single
                  pane it is just the app's view (no header), and either way
                  it is where a dragged chat can land and split. */}
              <SplitArea
                layout={layout}
                setLayout={setLayout}
                labelFor={paneLabel}
                renderPane={renderPane}
                onDropInPane={dropInPane}
                canSplit={panesCanSplit}
                onSaveLayout={() => setSavingLayout(true)}
                onRestoreLayout={applySavedLayout}
                // A saved "chat" pane comes back holding the chat on screen, so
                // the header can also say whose pane it is while it is split.
                currentChat={isChatView(view) ? view : null}
              />
              {/* The browser docked right: beside the chat, never instead
                  of it. The floating bar (on the homepage) and the sidebar
                  open and close it. */}
              {browserOpen && (
                <>
                  {/* The pane is as wide as the person left it, and this grip is how they
                      leave it: drag to widen or narrow, double-click for the default.
                      A separator, so the keyboard can move it too. */}
                  <div
                    className="bw-grip"
                    role="separator"
                    aria-orientation="vertical"
                    aria-label="Resize the browser pane"
                    aria-valuenow={browserWidth}
                    aria-valuemin={MIN_PANE_WIDTH}
                    tabIndex={0}
                    title="Drag to resize - double-click for the default width."
                    onPointerDown={startBrowserDrag}
                    onDoubleClick={() => {
                      setBrowserWidth(clampPaneWidth(DEFAULT_PANE_WIDTH, window.innerWidth))
                      savePaneWidth(DEFAULT_PANE_WIDTH)
                    }}
                    onKeyDown={(e) => {
                      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
                      e.preventDefault()
                      const step = e.key === 'ArrowLeft' ? -24 : 24
                      const next = clampPaneWidth(browserWidth + step, window.innerWidth)
                      setBrowserWidth(next)
                      savePaneWidth(next)
                    }}
                  />
                  <BrowserPane
                    width={browserWidth}
                    onClose={() => setBrowserOpen(false)}
                    askToShow={browserAsk}
                  />
                </>
              )}
            </div>
          </main>
          {paneMenu && (
            <ContextMenu
              x={paneMenu.x}
              y={paneMenu.y}
              items={paneMenuItems(paneMenu.leafId)}
              onClose={() => setPaneMenu(null)}
            />
          )}
          {/* The text-field menu: same engine, same keyboard handling, same
              click-away, and it closes when the field goes. */}
          {textMenu && (
            <ContextMenu
              x={textMenu.x}
              y={textMenu.y}
              items={textMenuItems(textMenu.el)}
              onClose={() => setTextMenu(null)}
            />
          )}
          {palette && (
            <CommandPalette
              recentChatIds={recentChatIds}
              onClose={() => setPalette(false)}
              onOpenChat={openInFocus}
              onOpenProject={newChatIn}
              onRace={() => setRacing(true)}
              raceFree={freePanes(layout)}
              panes={paneCount(layout)}
              paneCapacity={isMobile ? 1 : MAX_PANES}
              onSaveLayout={() => setSavingLayout(true)}
              onRestoreLayout={applySavedLayout}
              // A saved "chat" pane comes back holding the chat on screen, which
              // is the value the palette hands the same restore.
              currentChat={isChatView(view) ? view : null}
              // Renaming from the palette: the focused pane's chat, and the
              // opener the sidebar already uses (the dialog is hosted below).
              onRenameChat={openRename}
              onNewWithSetup={newChatWithSetup}
              focusedChatId={focusedChatId}
              onOpenMachine={(id) => {
                setOpenDevice({ id, nonce: Date.now() })
                goView('machines')
              }}
            />
          )}
          {racing && (
            <RaceModal
              free={freePanes(layout)}
              onClose={() => setRacing(false)}
              onStarted={startRace}
            />
          )}
          {savingLayout && (
            <SaveLayoutModal
              layout={layout}
              currentView={view}
              onClose={() => setSavingLayout(false)}
            />
          )}
          {sheet && <ShortcutSheet onClose={() => setSheet(false)} />}
          {pendingRestore && (
            <RestoreLayoutConfirm
              title={pendingRestore.ask.title}
              note={pendingRestore.ask.note}
              confirm={pendingRestore.ask.confirm}
              cancel={pendingRestore.ask.cancel}
              onConfirm={() => {
                restoreNow(pendingRestore.plan)
                setPendingRestore(null)
              }}
              onClose={() => setPendingRestore(null)}
            />
          )}
          {/* Renaming, hosted above every surface that offers it: the sidebar
              passes its row here, and the palette's row will too. */}
          {renaming && (
            <RenameChatModal
            title={
            chats[renaming.id]?.title ??
            (sessions ?? []).find((s) => s.id === renaming.id)?.title ??
            renaming.title
          }
              synced={renaming.synced}
              onClose={() => setRenaming(null)}
              onRename={(next) => renameTo(renaming.id, next)}
            />
          )}
          {/* The review surface for the chat's reported edits. It opens
              itself when the palette asks for it and needs no state here. It is
              mounted with the chat the keyboard and the palette call current
              (`focusedChatId`), not the primary pane's: with a split open the only
              review mount used to carry pane one's record while focus sat on pane
              two, and the palette's "Changes in this chat" then reviewed a
              transcript the person was not looking at. */}
          {(chats[focusedChatId] ?? chat) && <ChangesModal chat={chats[focusedChatId] ?? chat} />}
          {popup && (
            <ViewPopup
              label={{ settings: 'Settings', metrics: 'Metrics', mobile: 'Mobile' }[popup]}
              onClose={() => setPopup(null)}
              bare={isMobile && popup === 'settings'}
            >
              {popup === 'settings' ? (
                <SettingsView
                  onClose={() => setPopup(null)}
                  onReplay={() => {
                    setPopup(null)
                    replayOnboarding()
                  }}
                  onOpenChat={(id) => {
                    setPopup(null)
                    openSession(id)
                  }}
                />
              ) : popup === 'metrics' ? (
                <MetricsView />
              ) : (
                <MobileView
                  onOpenChat={(id) => {
                    setPopup(null)
                    openSession(id)
                  }}
                />
              )}
            </ViewPopup>
          )}
        </div>
      )}
      </div>
    </div>
  )

  return body()
}

export default function App() {
  return (
    <ConsoleProvider>
      <KeyboardProvider>
        <div className="app">
          <Shell />
        </div>
      </KeyboardProvider>
    </ConsoleProvider>
  )
}

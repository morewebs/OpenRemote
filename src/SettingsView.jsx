import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import {
  Archive,
  CaretLeft,
  CaretRight,
  Cloud,
  Globe,
  Info,
  Keyboard as KeyboardIcon,
  MagnifyingGlass,
  Palette,
  Plugs,
  Power,
  Robot,
  Shapes,
  Sparkle,
  TerminalWindow,
  X,
} from '@phosphor-icons/react'
import AboutModal from './AboutModal.jsx'
import McpServersSection from './McpServersSection.jsx'
import EndpointManager from './EndpointManager.jsx'
import { EndpointManagerModal } from './EndpointManager.jsx'
import DeleteChatModal from './DeleteChatModal.jsx'
import SignInModal from './SignInModal.jsx'
import PickerMenu from './PickerMenu.jsx'
import Toggle from './Toggle.jsx'
import { SECTIONS, SETTING_GROUPS, SETTINGS_ENTRIES, dynamicEntries, resultToOpen, searchSettings, visibleSections } from './settings-search.js'
import { projectName } from './projects.js'
import { chatTitle } from './shape.js'
import { armRow, clearRow, rowIsArmed } from './row-confirm.js'
import { call as callShell, clearHistory, getState as getBrowserState, removeBookmark, subscribe as subscribeBrowser } from './browser-store.js'
import { useBack } from './use-back.js'
import { useConsole } from './state/console.jsx'
import { signedIn } from './cloud.js'
import { isMobile } from './platform.js'
import { archivedSessions } from './archive.js'
import { harnessName } from './harness-names.js'
import { SIGNIN_HARNESSES, pickable } from './harness-manager.js'
import { loadDefaults, saveDefaults } from './defaults.js'
import { findModel, modelDisplayName, prettyModelName } from './model-display.js'
import { modelMaker } from './model-maker.js'
import { ModelMark } from './model-marks.jsx'
import {
  loadPrefs,
  savePrefs,
  applyReduceMotion,
  applyDensity,
  applyPrivacyMode,
} from './settings.js'
import { hasTauri } from './pick-folder.js'
import { errorCopy } from './error-copy.js'
import { usePrivacyMode } from './privacy.js'
import { version } from '../package.json'
import './devices.css'
import './panels.css'
import './settings.css'
import './keyboard.css'
import { useKeyboard, isMac } from './keyboard.jsx'
import { ACTIONS, GROUPS, captureRefusal, comboFromEvent, comboLabel, isOverridden } from './shortcuts.js'

// The harnesses whose own login command the daemon can relay - the
// Sign-in action renders only there (no dead UI elsewhere).

// The tab icon per section. The sections themselves and the search index
// live in settings-search.js, so a new tab is one entry there.
const SECTION_ICONS = {
  'new-tasks': Sparkle,
  appearance: Palette,
  startup: Power,
  keyboard: KeyboardIcon,
  cloud: Cloud,
  daemon: TerminalWindow,
  harnesses: Robot,
  'mcp-servers': Plugs,
  models: Shapes,
  browser: Globe,
  archived: Archive,
  'this-install': Info,
}

const MAX_RESULTS = 24

// The time a fact was read, said plainly. A row the console cannot re-read on
// focus says when it last looked rather than pretending to be live.
const checkedAt = (at) =>
  new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

// `onClose` is optional: the window instance (opened over the app) has a close
// control and passes one, the pane instance does not and hides it. Saying so
// with a default keeps the shell's prop check honest - it reads these
// declarations, and a required-looking prop that is never passed is how a
// call site goes unchecked.
export default function SettingsView({ onReplay, onOpenChat, onClose = null, onManualConnect = null }) {
  const privacy = usePrivacyMode()
  const {
    connection,
    capabilities,
    api,
    sessions,
    disconnect,
    modelsFor,
    cloud,
    signOutCloud,
    projects: projectsList,
    removeProject,
    chats,
    archived,
    mcpServers,
    setChatArchived,
    deleteChat,
  } = useConsole()
  const [deleting, setDeleting] = useState(null)
  // A profile that refuses storage leaves the archived flag where it was; the toggles
  // above report that refusal, and until now this section did not.
  const [archiveRefused, setArchiveRefused] = useState(false)
  const archivedRows = archivedSessions(sessions, archived)
  // What each endpoint table holds, handed up by EndpointManager so the search
  // can index an endpoint by the name its person gave it. The table owns the
  // only read of that route; this is a mirror, never a second fetch.
  const [endpointLists, setEndpointLists] = useState({})
  const reportEndpoints = useCallback(
    (harnessId, list) =>
      setEndpointLists((current) => (current[harnessId] === list ? current : { ...current, [harnessId]: list })),
    [],
  )
  const [confirmSignOut, setConfirmSignOut] = useState(false)
  const inCloud = signedIn(cloud)
  const harnessList = capabilities?.harnesses ?? []
  const available = (harnessList ?? []).filter(pickable)
  const [defaults, setDefaults] = useState(() => loadDefaults())
  const [models, setModels] = useState([])
  const [picker, setPicker] = useState(null)
  const [pickerLeaving, setPickerLeaving] = useState(false)
  const pickerLeavingRef = useRef(false)
  const [managingEndpoints, setManagingEndpoints] = useState(false)
  const [about, setAbout] = useState(false)
  const [signInFor, setSignInFor] = useState(null)
  const [prefs, setPrefs] = useState(() => loadPrefs())
  const [confirmDisconnect, setConfirmDisconnect] = useState(false)
  // Keyboard: the action waiting for its new keys, and the last move's
  // note ("moved from New chat") under the action that took the key.
  const kb = useKeyboard()
  const [capturing, setCapturing] = useState(null)
  const [moved, setMoved] = useState(null)
  // Why the last key pressed in the capture was refused, shown under the row
  // that is waiting for keys. Cleared when the capture ends or a key is taken.
  const [refused, setRefused] = useState(null)

  // While an action waits for its keys, every keystroke is the binding:
  // Esc cancels, Backspace or Delete clears the key, a lone modifier
  // waits for the rest. A click anywhere else cancels too.
  useEffect(() => {
    if (!capturing || !kb) return
    kb.setCapturing(true)
    const onKey = (e) => {
      e.preventDefault()
      e.stopPropagation()
      const bare = !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey
      if (e.key === 'Escape' && bare) {
        setRefused(null)
        setCapturing(null)
        return
      }
      if ((e.key === 'Backspace' || e.key === 'Delete') && bare) {
        kb.unbind(capturing)
        setMoved(null)
        setRefused(null)
        setCapturing(null)
        return
      }
      const combo = comboFromEvent(e, isMac)
      if (!combo) return
      // A key the dispatcher would never deliver while the user types is not
      // a binding this page may store: it would draw a key that never runs
      // where the message is written. The capture stays open so the next
      // keypress can be the one that works.
      const refusal = captureRefusal(combo)
      if (refusal) {
        setRefused(refusal)
        return
      }
      setRefused(null)
      const displaced = kb.rebind(capturing, combo)
      setMoved(
        displaced.length
          ? { id: capturing, from: displaced.map((id) => ACTIONS.find((a) => a.id === id)?.label ?? id).join(', ') }
          : null,
      )
      setCapturing(null)
    }
    const onDown = () => {
      setRefused(null)
      setCapturing(null)
    }
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('mousedown', onDown, true)
    return () => {
      kb.setCapturing(false)
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('mousedown', onDown, true)
    }
  }, [capturing, kb])
  const defaultHarness = available.find((h) => h.id === defaults.harness) ?? available[0] ?? null

  // The model row only renders where the default harness advertises -
  // the same rule the composer holds.
  useEffect(() => {
    let cancelled = false
    if (!defaultHarness) {
      setModels([])
      return
    }
    modelsFor(defaultHarness.id).then((list) => {
      if (cancelled) return
      const rows = list ?? []
      setModels(rows)
      const current = loadDefaults()
      const known = rows.some((m) => m.model === current.model)
      const first = rows.find((m) => m.is_default)?.model ?? rows[0]?.model ?? null
      if (!known && first) {
        saveDefaults({ model: first })
        setDefaults(loadDefaults())
      }
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [defaultHarness?.id, connection.state])

  const set = (partial) => {
    saveDefaults(partial)
    setDefaults(loadDefaults())
    setPicker(null)
  }

  // The effort row offers only the default model's own tiers - the same
  // rule the composer holds. A default model without tiers, or a saved
  // tier the model doesn't offer, means no row: never a dead selection.
  const defaultModel =
    models.find((m) => m.model === defaults.model) ??
    models.find((m) => m.is_default) ??
    null
  const effortTiers = defaultModel?.reasoning_efforts ?? []
  const savedEffort = defaults.efforts?.[defaultHarness?.id] ?? null
  const chosenEffort = effortTiers.includes(savedEffort) ? savedEffort : null
  // The permission row offers only the default harness's own words -
  // the same rule the composer holds. No words, no row. The saved word is
  // shown only while the harness still offers it, so a word from another
  // harness reads as unset rather than as a choice that would not apply.
  const permissionWords = defaultHarness?.permission_modes ?? []
  const savedPermission = defaults.permissions?.[defaultHarness?.id] ?? null
  const chosenPermission = permissionWords.some((w) => w.w === savedPermission) ? savedPermission : null

  // The one line for a setting whose write did not land: a startup row the
  // shell refused, or a preference the storage refused. Both used to fail
  // silently - the switch simply did not move - which reads as a broken app
  // rather than as a machine that said no.
  const [startupNote, setStartupNote] = useState(null)

  const setPref = (partial) => {
    const landed = savePrefs(partial)
    setPrefs(loadPrefs())
    // A pref chosen from a picker closes it, the way `set` does for the New
    // tasks rows: leaving the menu open read as "the choice did not land".
    setPicker(null)
    // The rows re-read the stored value, so a refused write leaves them showing
    // the value from before. The note is what tells the press was heard and not
    // kept; a write that landed clears it again.
    setStartupNote(landed ? null : 'The console could not save that setting, so it is unchanged.')
    return landed
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

  const openPicker = (kind) => (e) => {
    if (picker?.kind === kind) {
      hidePicker()
      return
    }
    if (pickerLeavingRef.current) return
    const btn = e.currentTarget.getBoundingClientRect()
    if (btn.width > 0) {
      setPicker({
        kind,
        x: Math.max(8, Math.round(btn.right - 264)),
        y: Math.round(btn.bottom + 4),
      })
      return
    }
    setPicker({ kind, x: e.clientX, y: e.clientY })
  }

  // The document attribute follows the stored preference, never ahead of it: a
  // switch that reads on while the app is not is worse than one that stayed
  // off, and a failed save has to leave both of them where they were.
  const toggleReduce = () => {
    const value = !prefs.reduce_motion
    if (!setPref({ reduce_motion: value })) return
    applyReduceMotion(document, value)
  }

  const togglePrivacy = () => {
    const value = !prefs.privacy_mode
    if (!setPref({ privacy_mode: value })) return
    applyPrivacyMode(document, value)
  }

  // The Windows startup toggle: the shell's autostart plugin writes the
  // Run key. The fact is asked once on open; toggling flips it through
  // the plugin and re-reads. Browser dev has no shell - the row never
  // renders there.
  // A tick that moves whenever the window is looked at again. Two of these
  // rows read a fact that can change outside this panel (the tray setting, the
  // daemon's address), so they ask again on focus instead of showing what was
  // true when Settings opened.
  const [focusTick, setFocusTick] = useState(0)
  useEffect(() => {
    const bump = () => {
      if (!document.hidden) setFocusTick((n) => n + 1)
    }
    window.addEventListener('focus', bump)
    document.addEventListener('visibilitychange', bump)
    return () => {
      window.removeEventListener('focus', bump)
      document.removeEventListener('visibilitychange', bump)
    }
  }, [])

  // The Windows startup toggle: the shell's autostart plugin writes the Run
  // key. This one is read once on open and not re-read on focus - the row says
  // when it was read, so a key changed outside the app reads as "older news"
  // rather than as a lie. Toggling re-reads it.
  const [autoStart, setAutoStart] = useState(null)
  const [autoStartAt, setAutoStartAt] = useState(null)
  useEffect(() => {
    if (!hasTauri || isMobile) return
    let cancelled = false
    ;(async () => {
      try {
        const { isEnabled } = await import('@tauri-apps/plugin-autostart')
        const enabled = await isEnabled()
        if (!cancelled) {
          setAutoStart(enabled)
          setAutoStartAt(Date.now())
        }
      } catch {
        /* the row stays absent */
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])
  const toggleAutoStart = async () => {
    if (autoStart == null) return
    const wanted = !autoStart
    setStartupNote(null)
    try {
      const { isEnabled, enable, disable } = await import('@tauri-apps/plugin-autostart')
      if (autoStart) await disable()
      else await enable()
      const enabled = await isEnabled()
      setAutoStart(enabled)
      setAutoStartAt(Date.now())
      // Windows can refuse the Run-key write without throwing: the plugin's own
      // read-back then still says what it said before. Say so, or the switch
      // reads as inert and the person cannot tell a locked-down machine from a
      // broken app.
      if (enabled !== wanted) {
        setStartupNote('Launch at login is unchanged: this computer did not let the startup entry change.')
      }
    } catch (err) {
      setStartupNote(errorCopy(err))
    }
  }

  // Keeping OpenRemote in the tray when the window closes: the shell's own
  // setting (on by default when this computer is a machine). Absent in the
  // browser, and where no tray can be shown. Re-asked on focus, because the
  // close behaviour can change it while Settings sits open behind another
  // window.
  const [tray, setTray] = useState(null)
  useEffect(() => {
    if (!hasTauri || isMobile) return
    let cancelled = false
    ;(async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const view = await invoke('tray_prefs')
        if (!cancelled) setTray(view)
      } catch {
        /* the row stays absent, or keeps the value it had */
      }
    })()
    return () => {
      cancelled = true
    }
  }, [focusTick])
  const toggleTray = async () => {
    if (!tray) return
    const wanted = !tray.keep_running
    setStartupNote(null)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      const next = await invoke('set_tray_prefs', { keepRunning: wanted })
      setTray(next)
      // The shell answers with the setting it holds. One that comes back the
      // other way round means the change did not take, and the row must not
      // look as though it did.
      if (next && typeof next.keep_running === 'boolean' && next.keep_running !== wanted) {
        setStartupNote('Keep running in the tray is unchanged: the shell did not take the change.')
      }
    } catch (err) {
      setStartupNote(errorCopy(err))
    }
  }

  // The shell restart: this side is a single invoke; the console heals
  const [restarting, setRestarting] = useState(false)
  const restartDaemon = async () => {
    if (!hasTauri || restarting) return
    setRestarting(true)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('daemon_restart')
    } catch {
      /* the poll reports the outcome either way */
    }
    setTimeout(() => setRestarting(false), 4000)
  }

  // The daemon's port and data dir, straight from the shell's own
  // record of the sidecar. Browser dev has no sidecar - no dead rows. Asked
  // again on a reconnect and on focus: a restart can move the port while the
  // window is in the background.
  const [daemonDetail, setDaemonDetail] = useState(null)
  useEffect(() => {
    if (!hasTauri) return
    let cancelled = false
    const ask = async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const info = await invoke('daemon_info')
        if (!cancelled && info?.url) setDaemonDetail(info)
      } catch {
        /* the row simply stays absent */
      }
    }
    ask()
    return () => {
      cancelled = true
    }
  }, [connection.state, focusTick])

  // Tabs and search: the panel shows one section at a time, and a query
  // replaces it with results gathered from every section this device has.
  const [tab, setTab] = useState(isMobile ? 'appearance' : 'new-tasks')
  // On a phone the list is the screen. A category opens over it, and a search
  // replaces it; one back ladder covers both, so a search is never a dead end:
  // clear the query, then leave the category, then leave Settings.
  const [category, setCategory] = useState(null)
  const [query, setQuery] = useState('')
  const phoneBack = () => {
    if (query) {
      setQuery('')
      return
    }
    if (category != null) {
      setCategory(null)
      return
    }
    onClose?.()
  }
  useBack(isMobile && (category != null || query !== ''), phoneBack)
  const [shortcutQuery, setShortcutQuery] = useState('')
  const [flash, setFlash] = useState(null)
  // The project waiting on a "remove?" answer. Removing one keeps its chats
  // and never touches the folder, but there is no undo, so the chip arms the
  // row first. row-confirm.js owns the rule (one armed row at a time, no id,
  // no arming) so this list and the sidebar cannot drift apart.
  const [armedProject, setArmedProject] = useState(null)
  // The armed project's name, for the sentence and the label that ask.
  const armedProjectName = armedProject
    ? projectName((projectsList ?? []).find((p) => p.id === armedProject))
    : null
  // The browser's own answers to "what is kept here": its store already holds
  // the bookmarks, the reading record and the download ledger on this
  // computer, so the section reads that store rather than a second copy. The
  // armed row is the same rule as the project's (one press to ask, one to
  // act), and the note carries the store's own words when an action finishes.
  const browserStore = useSyncExternalStore(subscribeBrowser, getBrowserState)
  const [armedBrowser, setArmedBrowser] = useState(null)
  const [browserNote, setBrowserNote] = useState(null)
  const bookmarkCount = browserStore.bookmarks.length
  const historyCount = browserStore.history.length
  const downloadCount = (browserStore.downloads ?? []).length
  const lastDownload = (browserStore.downloads ?? []).find((d) => d.path) ?? null
  // Clearing reads the store's own functions: the reading record has one
  // clear (its own), and a bookmark is removed by the same call the bar's
  // editor uses, so neither rule is restated here.
  const clearTheHistory = () => {
    clearHistory()
    setArmedBrowser(clearRow())
    setBrowserNote('History cleared.')
  }
  const clearTheBookmarks = () => {
    for (const mark of getBrowserState().bookmarks) removeBookmark(mark.id)
    setArmedBrowser(clearRow())
    setBrowserNote('Bookmarks cleared.')
  }
  // The shell's own call, the one the pane's download row makes: it reveals a
  // downloaded file in its folder. A download is not cleared from here, so the
  // row only shows the newest one where it already is.
  const showLastDownload = () => {
    if (!lastDownload) return
    callShell('browser_reveal', { path: lastDownload.path })
  }
  const panelRef = useRef(null)
  const searchRef = useRef(null)
  const shortcutSearchRef = useRef(null)
  const navRef = useRef(null)
  const tabs = useMemo(() => visibleSections(isMobile), [])
  // Lay the strip out the way the stylesheet does (settings.css turns it into a
  // horizontal row at 700px), so the tablist can announce what it really is.
  const [narrow, setNarrow] = useState(
    () => typeof window !== 'undefined' && window.matchMedia('(max-width: 700px)').matches,
  )
  useEffect(() => {
    const query = window.matchMedia('(max-width: 700px)')
    const onChange = () => setNarrow(query.matches)
    onChange()
    query.addEventListener('change', onChange)
    return () => query.removeEventListener('change', onChange)
  }, [])
  const active = tabs.some((s) => s.id === tab) ? tab : tabs[0].id
  const navGroups = useMemo(
    () =>
      SETTING_GROUPS.map((group) => ({
        ...group,
        sections: tabs.filter((section) => section.group === group.id),
      })).filter((group) => group.sections.length > 0),
    [tabs],
  )
  const sectionLabel = (id) => SECTIONS.find((s) => s.id === id)?.label ?? id

  // The rows whose list is daemon data are built by the module from these same
  // arrays (settings-search.js), so a seventh harness or a freshly archived
  // chat is searchable without an edit here or there.
  const index = useMemo(
    () => [
      ...SETTINGS_ENTRIES,
      ...dynamicEntries({ harnesses: harnessList, mcpServers, archived: archivedRows, chats, endpoints: endpointLists }),
    ],
    [harnessList, mcpServers, archivedRows, chats, endpointLists],
  )

  // A row that is not on screen (no model catalog, no tray, browser dev)
  // must not answer the search: a result that lands nowhere is dead UI.
  const absent = useMemo(() => {
    const keys = new Set()
    if (!models.length) keys.add('new-tasks-model')
    if (!effortTiers.length) keys.add('new-tasks-effort')
    if (!permissionWords.length) keys.add('new-tasks-permission')
    if (autoStart == null) keys.add('startup-launch')
    if (!tray?.available) keys.add('startup-tray')
    if (!daemonDetail?.url) keys.add('daemon-address')
    if (!hasTauri) keys.add('daemon-restart')
    // The connect row renders only while nothing is connected and only where
    // the shell can offer the form; the stale-key row only while there is a
    // stale key to clear.
    if (!onManualConnect || connection.state === 'connected') keys.add('daemon-connect')
    if (!(kb?.staleBindings?.length > 0)) keys.add('keyboard-stale')
    // "Reset every shortcut" is in the bar only while some key has been
    // changed with the defaults gone, there is nothing to reset.
    if (!(Object.keys(kb?.overrides ?? {}).length > 0)) keys.add('keyboard-reset')
    if (isMobile) keys.add('install-first-run')
    return keys
  }, [models, effortTiers, permissionWords, autoStart, tray, daemonDetail, onManualConnect, connection.state, kb])

  const searchable = useMemo(() => {
    const ids = new Set(tabs.map((s) => s.id))
    return index.filter((e) => ids.has(e.section) && !absent.has(e.key))
  }, [index, absent, tabs])
  const results = useMemo(() => searchSettings(query, searchable), [query, searchable])
  const shown = results.slice(0, MAX_RESULTS)
  const counts = useMemo(() => {
    const map = {}
    for (const entry of results) map[entry.section] = (map[entry.section] ?? 0) + 1
    return map
  }, [results])

  // The Keyboard tab's own filter: an action answers by its name, its
  // group, and both spellings of its key, and the row being rebound stays
  // on screen while its old keys stop matching.
  const keyboardFilter = shortcutQuery.trim().toLowerCase()
  const keyboardGroups = useMemo(() => {
    const words = keyboardFilter.split(/\s+/).filter(Boolean)
    return GROUPS.map((group) => ({
      ...group,
      actions: ACTIONS.filter((a) => {
        if (a.group !== group.id) return false
        if (!words.length || capturing === a.id) return true
        const combo = kb?.bindings?.[a.id] ?? null
        const hay = [
          a.label,
          group.label,
          combo ? comboLabel(combo, isMac) : '',
          combo ?? '',
          combo ? '' : 'no key unbound',
        ]
          .join(' ')
          .toLowerCase()
        return words.every((word) => hay.includes(word))
      }),
    })).filter((group) => group.actions.length > 0)
  }, [keyboardFilter, kb, capturing])
  const shortcutCount = keyboardGroups.reduce((sum, group) => sum + group.actions.length, 0)

  const selectTab = (id) => {
    setTab(id)
    if (isMobile) setCategory(id)
    setQuery('')
    setShortcutQuery('')
    setPicker(null)
    setFlash(null)
  }

  const openResult = (entry) => {
    setQuery('')
    setShortcutQuery('')
    setTab(entry.section)
    if (isMobile) setCategory(entry.section)
    setFlash(entry.key)
    setPicker(null)
  }

  const onTabKey = (e) => {
    const at = tabs.findIndex((s) => s.id === active)
    let next = null
    if (e.key === 'ArrowDown' || e.key === 'ArrowRight') next = (at + 1) % tabs.length
    else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') next = (at - 1 + tabs.length) % tabs.length
    else if (e.key === 'Home') next = 0
    else if (e.key === 'End') next = tabs.length - 1
    else return
    e.preventDefault()
    const id = tabs[next].id
    selectTab(id)
    // The tab buttons stay mounted, so moving focus now survives the
    // render the selection starts.
    navRef.current?.querySelector(`#stg-tab-${id}`)?.focus()
  }

  // Every new tab or query starts at the top; a chosen result then finds
  // its own row.
  useEffect(() => {
    if (panelRef.current) panelRef.current.scrollTop = 0
  }, [active, query])

  // Restart the fade when the visible section changes. Removing `hidden`
  // does not replay a CSS animation on its own.
  useEffect(() => {
    const panel = panelRef.current
    if (!panel) return
    panel.classList.remove('stg-switch')
    void panel.offsetWidth
    panel.classList.add('stg-switch')
  }, [active, query])

  useEffect(() => {
    const panel = panelRef.current
    if (!panel) return
    panel.querySelectorAll('.stg-found').forEach((el) => el.classList.remove('stg-found'))
    if (!flash || query) return
    const target = panel.querySelector(`[data-stg="${flash}"]`)
    if (target) {
      target.classList.add('stg-found')
      panel.focus({ preventScroll: true })
      target.scrollIntoView({ block: 'center' })
    }
  }, [flash, active, query])

  // "/" reaches the settings search from anywhere in the window, except
  // from a field that is already typing and while a key is being bound.
  useEffect(() => {
    const onKey = (e) => {
      if (e.key !== '/' || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return
      if (capturing || picker || deleting || about || signInFor) return
      const target = e.target
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
      ) {
        return
      }
      e.preventDefault()
      const field =
        active === 'keyboard' ? shortcutSearchRef.current ?? searchRef.current : searchRef.current
      field?.focus()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [capturing, picker, deleting, about, signInFor, active])

  const phoneHome = isMobile && category == null
  const phoneCategory = isMobile && category != null

  return (
    <div className={`settings${isMobile ? ' settings--phone' : ''}${phoneCategory ? ' is-category' : ''}`}>
      {isMobile && (
        <header className="stg-phone-bar">
          {(phoneCategory || query || onClose) && (
            <button
              type="button"
              className="stg-phone-back"
              onClick={phoneBack}
              aria-label={query ? 'Clear the search' : 'Back'}
            >
              <CaretLeft size={22} />
            </button>
          )}
          <h1 className="stg-phone-title">
            {phoneCategory ? sectionLabel(category) : query ? 'Search' : 'Settings'}
          </h1>
        </header>
      )}
      {!phoneCategory && (
      <aside className="stg-side">
        <label className="stg-search">
          <MagnifyingGlass size={14} className="stg-search-icon" />
          <input
            ref={searchRef}
            type="search"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value)
              setFlash(null)
            }}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                // Clear first, leave the field second. The window's own Escape
                // layer closes Settings; stopping the event here is what keeps
                // a clear from also closing the pane behind it.
                e.preventDefault()
                e.stopPropagation()
                if (query) setQuery('')
                else e.currentTarget.blur()
                return
              }
              // Down hands the keyboard to the rows (they are buttons, and a
              // focused row answers its own Enter); every other arrow, Home and
              // End stay the field's, for the caret.
              if (e.key === 'ArrowDown' && shown.length) {
                e.preventDefault()
                panelRef.current?.querySelector('.stg-result')?.focus()
                return
              }
              if (e.key === 'Enter') {
                // The same opener the rows call, through the one rule that
                // decides which row Enter means - so a click and a keystroke
                // cannot land in different places.
                const entry = resultToOpen(shown)
                if (!entry) return
                e.preventDefault()
                openResult(entry)
              }
            }}
            placeholder="Search settings"
            aria-label="Search settings"
            spellCheck={false}
          />
          {query ? (
            <button
              type="button"
              className="stg-search-clear"
              onClick={() => setQuery('')}
              title="Clear the search"
              aria-label="Clear the search"
            >
              <X size={11} weight="bold" />
            </button>
          ) : null}
        </label>
        <nav
          ref={navRef}
          className="stg-nav"
          role="tablist"
          aria-label="Settings sections"
          // The strip is a vertical rail on a wide window and a horizontal row
          // under 700px (settings.css), so the announced orientation follows the
          // same breakpoint rather than claiming vertical in both.
          aria-orientation={narrow ? 'horizontal' : 'vertical'}
          hidden={isMobile && !!query}
          onKeyDown={onTabKey}
        >
          {navGroups.map((group) => (
            <div key={group.id} className="stg-nav-group">
              <div className="stg-nav-group-label">{group.label}</div>
              {group.sections.map((s) => {
                const Icon = SECTION_ICONS[s.id] ?? Info
                const count = counts[s.id] ?? 0
                return (
                  <button
                    key={s.id}
                    type="button"
                    id={`stg-tab-${s.id}`}
                    role="tab"
                    aria-selected={active === s.id}
                    aria-controls={`stg-${s.id}`}
                    tabIndex={active === s.id ? 0 : -1}
                    className={active === s.id ? 'stg-nav-btn on' : 'stg-nav-btn'}
                    onClick={() => selectTab(s.id)}
                  >
                    <Icon size={isMobile ? 22 : 15} className="stg-nav-icon" />
                    <span className="stg-nav-label">{s.label}</span>
                    {isMobile && <CaretRight size={16} className="stg-nav-chevron" />}
                    {query && count > 0 && (
                      <span className="stg-nav-count" aria-hidden="true">
                        {count}
                      </span>
                    )}
                  </button>
                )
              })}
            </div>
          ))}
        </nav>
      </aside>
      )}
      {(!phoneHome || query) && (
      <div className="panel stg-panel" ref={panelRef} tabIndex={-1}>
        <div
          className="stg-results-view"
          role="region"
          aria-label="Search results"
          hidden={!query}
          onKeyDown={(e) => {
            // A query's Esc clears the search even after the arrow keys
            // moved focus into the results; it never closes the window.
            if (e.key === 'Escape' && query) {
              e.preventDefault()
              e.stopPropagation()
              setQuery('')
              searchRef.current?.focus()
            }
          }}
        >
          <header className="stg-head">
            <h2 className="stg-title">Search results</h2>
            <p className="stg-lead" aria-live="polite">
              {results.length
                ? `${results.length} ${results.length === 1 ? 'setting matches' : 'settings match'} "${query.trim()}".`
                : ''}
            </p>
          </header>
          {results.length === 0 ? (
            <div className="stg-empty">
              <p className="stg-empty-name">Nothing matches "{query.trim()}".</p>
              <p className="stg-empty-detail">
                Try a section name, or a word like "motion", "shortcut", or "daemon".
              </p>
            </div>
          ) : (
            <>
              <div className="stg-result-list">
                {shown.map((entry) => (
                  <button
                    key={entry.key}
                    type="button"
                    className="stg-result"
                    onClick={() => openResult(entry)}
                  >
                    <span className="stg-result-main">
                      {/* Behind privacy mode, like every other place those strings appear:
                          an archived chat's title or a folder path was blurred in the list
                          one tab over and readable here the moment a search was typed. */}
                      <span className="stg-result-name pv">{entry.label}</span>
                      <span className="stg-result-detail pv">{entry.detail}</span>
                    </span>
                    <span className="stg-result-where">{sectionLabel(entry.section)}</span>
                  </button>
                ))}
              </div>
              {results.length > shown.length && (
                <p className="stg-more">
                  Showing the first {shown.length}. Refine the search to narrow it.
                </p>
              )}
            </>
          )}
        </div>

        {!isMobile && (
          <section
            className="stg-section"
            id="stg-new-tasks"
            role="tabpanel"
            aria-labelledby="stg-tab-new-tasks"
            data-stg="new-tasks"
            hidden={!!query || active !== 'new-tasks'}
          >
            <header className="stg-head">
              <h2 className="stg-title">New tasks</h2>
              <p className="stg-lead">What a new chat starts with. The composers still change their minds.</p>
            </header>
            <div className="pn-list pn-list--tight">
              <div className="pn-row" data-stg="new-tasks-harness">
                <div>
                  <div className="pn-name">Harness</div>
                  <div className="pn-detail">What a new chat starts on.</div>
                </div>
                <button
                  type="button"
                  className="pn-btn"
                  data-unset={!defaultHarness || undefined}
                  data-picker="settings" onClick={openPicker('harness')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'harness'}
                >
                  {defaultHarness?.name ?? 'No harness installed'}
                </button>
                {picker?.kind === 'harness' && (
                  <PickerMenu
                    label="Harness"
                    searchPlaceholder="Search harnesses"
                    items={available.map((h) => ({ id: h.id, name: h.name }))}
                    groups={null}
                    selectedId={defaultHarness?.id ?? ''}
                    onChoose={(id) => set({ harness: id, model: null })}
                    leaving={pickerLeaving}
                      onClose={hidePicker}
                    anchor={{ left: picker.x, top: picker.y }}
                  />
                )}
              </div>
              {models.length > 0 && (
                <div className="pn-row" data-stg="new-tasks-model">
                  <div>
                    <div className="pn-name">Model</div>
                    <div className="pn-detail">{defaultHarness?.name}'s own catalog.</div>
                  </div>
                  <button
                    type="button"
                    className="pn-btn"
                    data-picker="settings" onClick={openPicker('model')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'model'}
                  >
                    {(() => {
                      // The chosen model's own maker mark, where the catalog
                      // names one; the harness default row carries the
                      // default model's, not none.
                      const shownModel = defaults.model && models.some((m) => m.model === defaults.model)
                        ? findModel(models, defaults.model)
                        : null
                      const row = shownModel ?? models.find((m) => m.is_default) ?? null
                      const maker = modelMaker(defaultHarness?.id, row?.model, row?.resolved_model)
                      return maker ? <ModelMark maker={maker} size={14} /> : null
                    })()}
                    {defaults.model && models.some((m) => m.model === defaults.model)
                      ? modelDisplayName(models, defaults.model)
                      : `${defaultHarness?.name} default`}
                  </button>
                  {picker?.kind === 'model' && (
                    <PickerMenu
                      label="Model"
                      searchPlaceholder="Search models"
                      wide
                      items={models.map((m) => ({
                        id: m.model,
                        name: prettyModelName(m.display_name ?? m.model),
                        resolved_model: m.resolved_model,
                      }))}
                      groups={null}
                      selectedId={defaults.model ?? ''}
                      onChoose={(id) => set({ model: id })}
                      leaving={pickerLeaving}
                      onClose={hidePicker}
                      anchor={{ left: picker.x, top: picker.y }}
                      renderIcon={(m) => (
                        <ModelMark maker={modelMaker(defaultHarness?.id, m.id, m.resolved_model)} size={14} />
                      )}
                      footer={
                        defaultHarness?.custom_endpoint_supported ? (
                          <button
                            type="button"
                            onClick={() => {
                              setPicker(null)
                              setManagingEndpoints(true)
                            }}
                          >
                            Manage models…
                          </button>
                        ) : null
                      }
                    />
                  )}
                </div>
              )}
              {effortTiers.length > 0 && (
                <div className="pn-row" data-stg="new-tasks-effort">
                  <div>
                    <div className="pn-name">Effort</div>
                    <div className="pn-detail">{defaultHarness?.name}'s own tier words for {prettyModelName(defaultModel?.display_name ?? defaultModel?.model)}.</div>
                  </div>
                  <button
                    type="button"
                    className="pn-btn"
                    data-picker="settings" onClick={openPicker('effort')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'effort'}
                  >
                    {chosenEffort ?? `${defaultHarness?.name} default`}
                  </button>
                  {picker?.kind === 'effort' && (
                    <PickerMenu
                      label="Effort"
                      searchPlaceholder="Search efforts"
                      items={effortTiers.map((t) => ({ id: t, name: t }))}
                      groups={null}
                      selectedId={chosenEffort ?? ''}
                      onChoose={(id) =>
                        set({ efforts: { ...defaults.efforts, [defaultHarness.id]: id } })
                      }
                      leaving={pickerLeaving}
                      onClose={hidePicker}
                      anchor={{ left: picker.x, top: picker.y }}
                    />
                  )}
                </div>
              )}
              {permissionWords.length > 0 && (
                <div className="pn-row" data-stg="new-tasks-permission">
                  <div>
                    <div className="pn-name">Permission</div>
                    <div className="pn-detail">
                      What {defaultHarness?.name} may do without asking, in its own words.
                    </div>
                  </div>
                  <button
                    type="button"
                    className="pn-btn"
                    data-picker="settings" onClick={openPicker('permission')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'permission'}
                  >
                    {chosenPermission ?? `${defaultHarness?.name} default`}
                  </button>
                  {picker?.kind === 'permission' && (
                    <PickerMenu
                      label="Permission"
                      searchPlaceholder="Search permission words"
                      items={permissionWords.map((word) => ({ id: word.w, name: word.l }))}
                      groups={null}
                      selectedId={chosenPermission ?? ''}
                      onChoose={(id) =>
                        set({ permissions: { ...defaults.permissions, [defaultHarness.id]: id } })
                      }
                      leaving={pickerLeaving}
                      onClose={hidePicker}
                      anchor={{ left: picker.x, top: picker.y }}
                      renderSubline={(item) =>
                        item.id === defaultHarness?.default_permission ? (
                          <span className="nc-item-sub">
                            {defaultHarness?.name} runs this by default
                          </span>
                        ) : null
                      }
                    />
                  )}
                </div>
              )}
              <div
                className="pn-row"
                data-stg="new-tasks-projects"
                // Focus or a click leaving the row is a decision to keep it:
                // the armed project never survives a look away.
                onBlur={(e) => {
                  if (armedProject && !e.currentTarget.contains(e.relatedTarget)) {
                    setArmedProject(clearRow())
                  }
                }}
              >
                <div>
                  <div className="pn-name">Projects</div>
                  <div className="pn-detail">
                    {armedProject != null
                      ? `Remove “${armedProjectName}”? The project goes; its chats move to Chats and the folder on disk is never touched.`
                      : projectsList.length
                        ? 'The folders whose chats group under their project. Removing a project keeps every chat.'
                        : 'No projects yet. Add one from the home screen.'}
                  </div>
                </div>
                <div className="pn-actions">
                  {/* The trash that used to sit here removed a project on the
                      first click, with no undo. It asks once now, through the
                      same armed-row rules the sidebar and the panes use, so
                      one project at a time can be waiting on an answer. */}
                  {armedProject != null ? (
                    <>
                      <button
                        type="button"
                        className="pn-btn"
                        autoFocus
                        onClick={() => setArmedProject(clearRow())}
                      >
                        Keep
                      </button>
                      <button
                        type="button"
                        className="pn-btn pn-btn--danger"
                        aria-label={`Remove the project ${armedProjectName}`}
                        onKeyDown={(e) => {
                          if (e.key === 'Escape') {
                            e.preventDefault()
                            setArmedProject(clearRow())
                          }
                        }}
                        onClick={() => {
                          setArmedProject(clearRow())
                          removeProject(armedProject)
                        }}
                      >
                        Remove it
                      </button>
                    </>
                  ) : (
                    projectsList.map((p) => (
                      <button
                        key={p.id}
                        type="button"
                        className="pn-btn"
                        title={privacy ? 'Remove the project - it asks once, and its chats stay' : `Remove the project - it asks once, and its chats stay. Folder: ${p.folders[0]}`}
                        onClick={() => setArmedProject((current) => armRow(current, p.id))}
                      >
                        <span className="pv">{projectName(p)}</span>
                        {/* The cross sits in its own span so it can be spaced: run
                            straight after the name it read as part of it
                            ("Obsidian×"), while it means something separate. */}
                        <span className="pn-chip-x" aria-hidden="true">
                          ✕
                        </span>
                      </button>
                    ))
                  )}
                </div>
              </div>
            </div>
          </section>
        )}

        <section
          className="stg-section"
          id="stg-appearance"
          role="tabpanel"
          aria-labelledby="stg-tab-appearance"
          data-stg="appearance"
          hidden={!!query || active !== 'appearance'}
        >
          <header className="stg-head">
            <h2 className="stg-title">Appearance</h2>
            <p className="stg-lead">Space, not themes - the dark theme is the contract.</p>
          </header>
          <div className="pn-list pn-list--tight">
            <div className="pn-row" data-stg="appearance-density">
              <div>
                <div className="pn-name">Compact density</div>
                <div className="pn-detail">
                  Pulls in the settings, list and sidebar rows. The transcript keeps its spacing.
                </div>
              </div>
              <Toggle
                checked={prefs.density === 'compact'}
                onChange={() => {
                  const value = prefs.density === 'compact' ? 'comfortable' : 'compact'
                  // The same rule as the two toggles below: the document
                  // follows a preference that was actually kept.
                  if (!setPref({ density: value })) return
                  applyDensity(document, value)
                }}
                label="Compact density"
              />
            </div>
            <div className="pn-row" data-stg="appearance-motion">
              <div>
                <div className="pn-name">Reduce motion</div>
                <div className="pn-detail">Stops the running-dot breathe and picker transitions.</div>
              </div>
              <Toggle checked={prefs.reduce_motion} onChange={toggleReduce} label="Reduce motion" />
            </div>
            <div className="pn-row" data-stg="appearance-privacy">
              <div>
                <div className="pn-name">Privacy mode</div>
                <div className="pn-detail">
                  For recordings and streams: blurs chat titles, projects, paths, device names, your
                  email and the transcript. What you type stays readable.
                </div>
              </div>
              <Toggle checked={prefs.privacy_mode} onChange={togglePrivacy} label="Privacy mode" />
            </div>
          </div>
          {/* The same line as the Startup rows', where a preference's own press
              happens: the storage can refuse a write on this tab too, and a
              sentence on another tab would be no sentence at all. */}
          {startupNote && (
            <p className="pn-error" role="status">
              {startupNote}
            </p>
          )}
        </section>

        {!isMobile && (
          <section
            className="stg-section"
            id="stg-startup"
            role="tabpanel"
            aria-labelledby="stg-tab-startup"
            data-stg="startup"
            hidden={!!query || active !== 'startup'}
          >
            <header className="stg-head">
              <h2 className="stg-title">Startup</h2>
              <p className="stg-lead">What the app does when it opens.</p>
            </header>
            <div className="pn-list pn-list--tight">
              <div className="pn-row" data-stg="startup-reopen">
                <div>
                  <div className="pn-name">Reopen</div>
                  <div className="pn-detail">The view you left, or always New chat.</div>
                </div>
                <button
                  type="button"
                  className="pn-btn"
                  data-picker="settings" onClick={openPicker('startup')}
                  aria-haspopup="listbox"
                  aria-expanded={picker?.kind === 'startup'}
                >
                  {prefs.startup_view === 'last' ? 'Last view' : 'New chat'}
                </button>
                {picker?.kind === 'startup' && (
                  <PickerMenu
                    label="Reopen"
                    searchPlaceholder="Search"
                    items={[
                      { id: 'new', name: 'New chat' },
                      { id: 'last', name: 'Last view' },
                    ]}
                    groups={null}
                    selectedId={prefs.startup_view}
                    onChoose={(id) => setPref({ startup_view: id })}
                    leaving={pickerLeaving}
                      onClose={hidePicker}
                    anchor={{ left: picker.x, top: picker.y }}
                  />
                )}
              </div>
              {autoStart != null && (
                <div className="pn-row" data-stg="startup-launch">
                  <div>
                    <div className="pn-name">Launch at login</div>
                    <div className="pn-detail">
                      Starts OpenRemote when you sign in to this computer.
                      {autoStartAt ? ` Checked at ${checkedAt(autoStartAt)}.` : ''}
                    </div>
                  </div>
                  <Toggle checked={autoStart} onChange={toggleAutoStart} label="Launch at login" />
                </div>
              )}
              {tray?.available && (
                <div className="pn-row" data-stg="startup-tray">
                  <div>
                    <div className="pn-name">Keep running in the tray</div>
                    <div className="pn-detail">
                      {tray.machine
                        ? 'Closing the window leaves OpenRemote running, so your other devices can still reach this one. Quit from the tray icon.'
                        : 'Closing the window leaves OpenRemote running, so chats here keep going. Quit from the tray icon.'}
                    </div>
                  </div>
                  <Toggle
                    checked={tray.keep_running}
                    onChange={toggleTray}
                    label="Keep running in the tray"
                  />
                </div>
              )}
              {/* One line under the startup rows for either failure: a row the
                  shell refused, or a preference the storage refused. */}
              {startupNote && (
                <p className="pn-error" role="status">
                  {startupNote}
                </p>
              )}
            </div>
          </section>
        )}

        {!isMobile && kb && (
          <section
            className="stg-section"
            id="stg-keyboard"
            role="tabpanel"
            aria-labelledby="stg-tab-keyboard"
            data-stg="keyboard"
            hidden={!!query || active !== 'keyboard'}
          >
            <header className="stg-head">
              <h2 className="stg-title">Keyboard</h2>
              <p className="stg-lead">
                Click an action, then press its new keys. A key another action holds moves here.
                Backspace clears a key, Esc cancels.
              </p>
            </header>
            <div className="stg-kb-bar">
              <label className="stg-kb-filter">
                <MagnifyingGlass size={13} className="stg-search-icon" />
                <input
                  ref={shortcutSearchRef}
                  type="search"
                  value={shortcutQuery}
                  onChange={(e) => setShortcutQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') {
                      e.preventDefault()
                      e.stopPropagation()
                      if (shortcutQuery) setShortcutQuery('')
                      else e.currentTarget.blur()
                    }
                  }}
                  placeholder="Filter shortcuts"
                  aria-label="Filter keyboard shortcuts"
                  spellCheck={false}
                />
                {shortcutQuery && (
                  <button
                    type="button"
                    className="stg-kb-clear"
                    onClick={() => setShortcutQuery('')}
                    title="Clear the filter"
                    aria-label="Clear the filter"
                  >
                    <X size={11} weight="bold" />
                  </button>
                )}
              </label>
              {shortcutQuery && (
                <span className="stg-kb-count" aria-live="polite">
                  {shortcutCount} of {ACTIONS.length}
                </span>
              )}
              {Object.keys(kb.overrides).length > 0 && (
                <button
                  type="button"
                  className="pn-btn"
                  onClick={() => {
                    kb.resetAll()
                    setMoved(null)
                  }}
                  title="Puts every key back to its default and touches nothing else."
                >
                  Reset every shortcut
                </button>
              )}
            </div>
            {keyboardGroups.length === 0 ? (
              <div className="stg-empty">
                <p className="stg-empty-name">Nothing matches "{shortcutQuery.trim()}".</p>
                <p className="stg-empty-detail">
                  Search an action, a group, or a key like "Ctrl+N".
                </p>
              </div>
            ) : (
              keyboardGroups.map((group) => (
                <div key={group.id} className="pn-list pn-list--tight kb-set">
                  <div className="kb-group-label">{group.label}</div>
                  {group.actions.map((a) => (
                    <div className="pn-row" key={a.id} data-stg={`keyboard-${a.id}`}>
                      <div>
                        <div className="pn-name">{a.label}</div>
                        {moved?.id === a.id && <div className="pn-detail">Moved from {moved.from}.</div>}
                        {refused && capturing === a.id && <div className="pn-detail">{refused}</div>}
                      </div>
                      <div className="pn-actions">
                        {isOverridden(kb.overrides, a.id) && (
                          <button
                            type="button"
                            className="pn-btn"
                            onClick={() => {
                              kb.reset(a.id)
                              setMoved(null)
                            }}
                          >
                            Reset
                          </button>
                        )}
                        <button
                          type="button"
                          className={capturing === a.id ? 'pn-btn on' : 'pn-btn'}
                          // Its own mousedown would cancel the capture it starts.
                          onMouseDown={(e) => e.stopPropagation()}
                          onClick={() => setCapturing(capturing === a.id ? null : a.id)}
                        >
                          {capturing === a.id ? (
                            'Press keys…'
                          ) : kb.bindings[a.id] ? (
                            <kbd className="kb-keys">{comboLabel(kb.bindings[a.id], isMac)}</kbd>
                          ) : (
                            'No key'
                          )}
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              ))
            )}
            {kb.staleBindings?.length > 0 && (
              // A saved key whose action this build no longer has. Deliberately
              // outside the filtered groups: it is not an action, so a search
              // for actions must not hide it, and its "Saved" label is what
              // keeps it from reading as a match. It names no action - the
              // action is gone - so it shows the key and a way to forget it.
              <div className="pn-list pn-list--tight kb-set">
                <div className="kb-group-label">Saved</div>
                <div className="pn-row" data-stg="keyboard-stale">
                  <div>
                    <div className="pn-name">Saved shortcut for an action this build no longer has</div>
                    <div className="pn-detail">
                      {kb.staleBindings
                        .map((b) => (b.combo ? comboLabel(b.combo, isMac) : 'no key'))
                        .join(', ')}
                    </div>
                  </div>
                  <div className="pn-actions">
                    <button type="button" className="pn-btn" onClick={() => kb.clearStale()}>
                      Clear
                    </button>
                  </div>
                </div>
              </div>
            )}
          </section>
        )}

        <section
          className="stg-section"
          id="stg-cloud"
          role="tabpanel"
          aria-labelledby="stg-tab-cloud"
          data-stg="cloud"
          hidden={!!query || active !== 'cloud'}
        >
          <header className="stg-head">
            <h2 className="stg-title">Cloud</h2>
            <p className="stg-lead">Your moreweb account, and {isMobile ? "this phone's" : "this computer's"} place in it.</p>
          </header>
          <div className="pn-list pn-list--tight">
            <div className="pn-row" data-stg="cloud-account">
          <div>
            <div className="pn-name">
              {inCloud ? (
                cloud.account?.email ? (
                  <>Signed in as <span className="pv">{cloud.account.email}</span></>
                ) : (
                  'Signed in to your moreweb account'
                )
              ) : (
                'Not signed in'
              )}
            </div>
            <div className="pn-detail">
              {inCloud
                ? confirmSignOut
                  ? `Synced chats stay on ${isMobile ? 'this phone' : 'this computer'}; they stop syncing until you sign in again.`
                  : isMobile
                    ? `This phone is ${cloud.device?.name ?? 'here'}.`
                    : `This computer is ${cloud.device?.name ?? 'here'}, ${cloud.device?.kind === 'machine' ? 'a machine your other devices can run chats on' : 'a desktop'}.`
                : 'Switch to Cloud to sign in. Local chats never need an account.'}
            </div>
          </div>
          {inCloud &&
            (confirmSignOut ? (
              <div className="pn-actions">
                <button type="button" className="pn-btn" onClick={() => setConfirmSignOut(false)}>Cancel</button>
                <button
                  type="button"
                  className="pn-btn"
                  onClick={async () => {
                    setConfirmSignOut(false)
                    await signOutCloud()
                  }}
                >
                  Sign out
                </button>
              </div>
            ) : (
              <button type="button" className="pn-btn" onClick={() => setConfirmSignOut(true)}>
                Sign out
              </button>
            ))}
            </div>
          </div>
        </section>

        {!isMobile && (
          <>
          <section
            className="stg-section"
            id="stg-daemon"
            role="tabpanel"
            aria-labelledby="stg-tab-daemon"
            data-stg="daemon"
            hidden={!!query || active !== 'daemon'}
          >
            <header className="stg-head">
              <h2 className="stg-title">Daemon</h2>
              <p className="stg-lead">The local process the chats run through.</p>
            </header>
            <div className="pn-list pn-list--tight">
              <div className="pn-row" data-stg="daemon-connection">
                <div>
                  <div className="pn-name">{connection.state === 'connected' ? 'Connected' : 'Not connected'}</div>
                  <div className="pn-detail">
                  {connection.state === 'connected'
                    ? confirmDisconnect
                      ? 'The saved address and token are forgotten; your chats, plugins and settings stay.'
                      : 'Bearer token accepted.'
                    : connection.error ?? 'Start the app, or use Connect by address below.'}
                </div>
                </div>
                {/* A disabled button labelled with a dash read as a rendering
                    fault rather than as "nothing to forget". The detail line
                    above says why there is nothing to do, so the control is
                    absent until there is something to forget. */}
                {confirmDisconnect ? (
                  <div className="pn-actions">
                    <button type="button" className="pn-btn" onClick={() => setConfirmDisconnect(false)}>Cancel</button>
                    <button type="button" className="pn-btn" onClick={disconnect}>Disconnect</button>
                  </div>
                ) : connection.state === 'connected' ? (
                  <button
                    type="button"
                    className="pn-btn"
                    onClick={() => setConfirmDisconnect(true)}
                  >
                    Forget
                  </button>
                ) : null}
              </div>
              {/* Only while nothing is connected: the form it opens says the
                  daemon did not answer, which is not true of a connection
                  that works. Forget is the row above. */}
              {onManualConnect && connection.state !== 'connected' && (
                <div className="pn-row" data-stg="daemon-connect">
                  <div>
                    <div className="pn-name">Connect by address</div>
                    <div className="pn-detail">Type the address and token of a daemon you run yourself.</div>
                  </div>
                  <button type="button" className="pn-btn" onClick={onManualConnect}>
                    Connect
                  </button>
                </div>
              )}
              <div className="pn-row" data-stg="daemon-chats">
                <div>
                  <div className="pn-name">{(sessions ?? []).length} {(sessions ?? []).length === 1 ? 'chat' : 'chats'} on record</div>
                  <div className="pn-detail">Sessions survive daemon restarts; stopped ones can resume.</div>
                </div>
              </div>
              {daemonDetail?.url && (
                <div className="pn-row" data-stg="daemon-address">
                  <div>
                    <div className="pn-name">{daemonDetail.url.replace(/^https?:\/\//, '')}</div>
                    <div className="pn-detail">
                      {daemonDetail.data_dir ? <span className="pv">{daemonDetail.data_dir}</span> : 'The local address the console talks to.'}
                    </div>
                  </div>
                </div>
              )}
              {hasTauri && (
                <div className="pn-row" data-stg="daemon-restart">
                  <div>
                    <div className="pn-name">Restart</div>
                    <div className="pn-detail">
                      {restarting
                        ? 'The daemon is coming back; the console reconnects on its own.'
                        : 'Stops running chats (stopped ones can resume) and starts a fresh daemon.'}
                    </div>
                  </div>
                  <button
                    type="button"
                    className="pn-btn"
                    onClick={restartDaemon}
                    disabled={restarting}
                  >
                    {restarting ? 'Restarting' : 'Restart'}
                  </button>
                </div>
              )}
            </div>
          </section>
          <section
            className="stg-section"
            id="stg-harnesses"
            role="tabpanel"
            aria-labelledby="stg-tab-harnesses"
            data-stg="harnesses"
            hidden={!!query || active !== 'harnesses'}
          >
            <header className="stg-head">
              <h2 className="stg-title">Harnesses</h2>
              <p className="stg-lead">What the daemon can drive on this machine. Install a harness's own CLI and it appears here.</p>
            </header>
            <div className="pn-list pn-list--tight">
              {harnessList.map((h) => (
                <div className="pn-row" key={h.id} data-stg={`harness-${h.id}`}>
                  <div>
                    <div className="pn-name">{harnessName(h.id)}</div>
                    <div className="pn-detail">{h.available ? <span className="pv">{h.path}</span> : 'This harness is not installed on this machine.'}</div>
                  </div>
                  {h.available && h.using_api_key && h.signed_in !== true ? (
                    <span className="pn-btn">Using API key</span>
                  ) : h.available && h.signed_in === false && SIGNIN_HARNESSES.has(h.id) ? (
                    <button type="button" className="pn-btn" onClick={() => setSignInFor(h.id)}>
                      Sign in
                    </button>
                  ) : (
                    <span className={`pn-btn${h.available && h.signed_in !== false ? ' on' : ''}`}>
                      {!h.available ? 'Missing' : h.signed_in === false ? 'Not signed in' : 'Ready'}
                    </span>
                  )}
                </div>
              ))}
            </div>
          </section>
          </>
        )}

        {!isMobile && (
          <section
            className="stg-section"
            id="stg-mcp-servers"
            role="tabpanel"
            aria-labelledby="stg-tab-mcp-servers"
            data-stg="mcp-servers"
            hidden={!!query || active !== 'mcp-servers'}
          >
            <header className="stg-head">
              <h2 className="stg-title">MCP servers</h2>
            </header>
            <McpServersSection />
          </section>
        )}

        {!isMobile && (
          <section
            className="stg-section"
            id="stg-models"
            role="tabpanel"
            aria-labelledby="stg-tab-models"
            data-stg="models"
            hidden={!!query || active !== 'models'}
          >
            <header className="stg-head">
              <h2 className="stg-title">Models</h2>
              <p className="stg-lead">
                Custom model endpoints on this computer - a self-hosted gateway or a relay
                on this network that speaks an OpenAI-compatible models API. The daemon
                holds the keys; the console never sees them.
              </p>
            </header>
            {(harnessList ?? []).filter((h) => h.custom_endpoint_supported).length === 0 ? (
              <div className="pn-list pn-list--tight">
                <div className="pn-row">
                  <div>
                    <div className="pn-name">No supported harness</div>
                    <div className="pn-detail">Custom endpoints arrive with claude - install it to add one.</div>
                  </div>
                </div>
              </div>
            ) : (
              (harnessList ?? [])
                .filter((h) => h.custom_endpoint_supported)
                .map((h) => (
                  <div key={h.id} data-stg={`models-${h.id}`}>
                    <h3 className="stg-subhead">{harnessName(h.id)}</h3>
                    <EndpointManager
                      harness={h.id}
                      onEndpoints={(list) => reportEndpoints(h.id, list)}
                    />
                  </div>
                ))
            )}
          </section>
        )}

        {!isMobile && (
          <section
            className="stg-section"
            id="stg-browser"
            role="tabpanel"
            aria-labelledby="stg-tab-browser"
            data-stg="browser"
            hidden={!!query || active !== 'browser'}
          >
            <header className="stg-head">
              <h2 className="stg-title">Browser</h2>
              <p className="stg-lead">
                What the built-in browser keeps on this computer, and where to take it
                back. Nothing here is sent anywhere.
              </p>
            </header>
            {/* The facts first: each names what is kept and where it lives, so
                a person can find out before they find the buttons. */}
            <div className="pn-list pn-list--tight">
              <div className="pn-row" data-stg="browser-bookmarks">
                <div>
                  <div className="pn-name">Bookmarks</div>
                  <div className="pn-detail">
                    {bookmarkCount} kept on this computer, in this console. Nothing
                    is sent anywhere.
                  </div>
                </div>
              </div>
              <div className="pn-row" data-stg="browser-history">
                <div>
                  <div className="pn-name">Pages read</div>
                  <div className="pn-detail">
                    {historyCount} in this window's reading record, on this computer
                    only. A private tab is never recorded.
                  </div>
                </div>
              </div>
              <div className="pn-row" data-stg="browser-profile">
                <div>
                  <div className="pn-name">Cookies and site data</div>
                  {/* No button: the shell clears site data for the tab you are
                      on, so this is a fact about where it lives rather than an
                      action that would do nothing from here. */}
                  <div className="pn-detail">
                    The profile the browser's tabs share on this computer: cookies,
                    site data, cache and service workers. It is cleared per site
                    from the browser's own menu, where the shell has the tab to
                    clear.
                  </div>
                </div>
              </div>
              <div className="pn-row" data-stg="browser-downloads">
                <div>
                  <div className="pn-name">Downloads</div>
                  <div className="pn-detail">
                    {downloadCount} file{downloadCount === 1 ? '' : 's'} the tabs
                    saved, in your Downloads folder. The list is the browser's own
                    Downloads page, which needs the browser open.
                  </div>
                </div>
              </div>
            </div>
            <div className="pn-list pn-list--tight">
              <div className="pn-row" data-stg="browser-history-clear">
                <div>
                  <div className="pn-name">Clear history</div>
                  <div className="pn-detail">Empties this window's reading record. It asks once.</div>
                </div>
                {rowIsArmed(armedBrowser, 'browser-history-clear') ? (
                  <button
                    type="button"
                    className="pn-btn pn-btn--danger"
                    onClick={clearTheHistory}
                    onBlur={() => setArmedBrowser(clearRow())}
                    onKeyDown={(e) => {
                      if (e.key === 'Escape') {
                        e.preventDefault()
                        setArmedBrowser(clearRow())
                      }
                    }}
                  >
                    Clear it
                  </button>
                ) : (
                  <button
                    type="button"
                    className="pn-btn"
                    disabled={historyCount === 0}
                    title={historyCount === 0 ? 'Nothing has been read in this window yet.' : undefined}
                    onClick={() => setArmedBrowser((current) => armRow(current, 'browser-history-clear'))}
                  >
                    Clear
                  </button>
                )}
              </div>
              <div className="pn-row" data-stg="browser-bookmarks-clear">
                <div>
                  <div className="pn-name">Clear bookmarks</div>
                  <div className="pn-detail">Removes every kept page. It asks once.</div>
                </div>
                {rowIsArmed(armedBrowser, 'browser-bookmarks-clear') ? (
                  <button
                    type="button"
                    className="pn-btn pn-btn--danger"
                    onClick={clearTheBookmarks}
                    onBlur={() => setArmedBrowser(clearRow())}
                    onKeyDown={(e) => {
                      if (e.key === 'Escape') {
                        e.preventDefault()
                        setArmedBrowser(clearRow())
                      }
                    }}
                  >
                    Clear them
                  </button>
                ) : (
                  <button
                    type="button"
                    className="pn-btn"
                    disabled={bookmarkCount === 0}
                    title={bookmarkCount === 0 ? 'Nothing is kept yet.' : undefined}
                    onClick={() => setArmedBrowser((current) => armRow(current, 'browser-bookmarks-clear'))}
                  >
                    Clear
                  </button>
                )}
              </div>
              <div className="pn-row" data-stg="browser-downloads-open">
                <div>
                  <div className="pn-name">Show the last download</div>
                  <div className="pn-detail">Opens the folder of the newest finished download.</div>
                </div>
                <button
                  type="button"
                  className="pn-btn"
                  disabled={!lastDownload}
                  title={lastDownload ? lastDownload.path : 'No download has finished yet.'}
                  onClick={showLastDownload}
                >
                  Show
                </button>
              </div>
            </div>
            {browserNote && (
              <p className="stg-note" role="status">
                {browserNote}
              </p>
            )}
          </section>
        )}

        <section
          className="stg-section"
          id="stg-archived"
          role="tabpanel"
          aria-labelledby="stg-tab-archived"
          data-stg="archived"
          hidden={!!query || active !== 'archived'}
        >
          <header className="stg-head">
            <h2 className="stg-title">Archived chats</h2>
            <p className="stg-lead">
              Hidden on this computer only. They keep running and syncing; Delete removes a synced
              chat from your other devices the next time each one syncs, and a private chat can't be
              deleted.
            </p>
          </header>
          {archiveRefused && (
            <p className="stg-note" role="status">
              This computer refused to save that change, so the chat is still archived.
            </p>
          )}
          <div className="pn-list pn-list--tight">
            {archivedRows.length === 0 ? (
              <div className="pn-row">
                <div>
                  <div className="pn-name">Nothing archived</div>
                  <div className="pn-detail">Archive a chat from the sidebar to tuck it away here.</div>
                </div>
              </div>
            ) : (
              archivedRows.map((s) => (
                <div className="pn-row" key={s.id} data-stg={`archived-${s.id}`}>
                  <div>
                    <div className="pn-name pv">{chatTitle({ title: chats[s.id]?.title ?? s.title })}</div>
                    <div className="pn-detail">
                      {harnessName(s.harness)}
                      {s.workspace ? <> · <span className="pv">{s.workspace}</span></> : ''}
                    </div>
                  </div>
                  <button type="button" className="pn-btn" onClick={() => onOpenChat(s.id)}>Open</button>
                  <button
                    type="button"
                    className="pn-btn"
                    data-stg="archived-restore-action"
                    onClick={() => setArchiveRefused(!setChatArchived(s.id, false))}
                  >
                    Restore
                  </button>
                  {s.executor && (
                    <button
                      type="button"
                      className="pn-btn pn-btn--danger"
                      data-stg="archived-delete-action"
                      title="Delete this chat here; other devices delete their copy when they next sync."
                      onClick={() => setDeleting(s)}
                    >
                      Delete
                    </button>
                  )}
                </div>
              ))
            )}
          </div>
        </section>

        <section
          className="stg-section"
          id="stg-this-install"
          role="tabpanel"
          aria-labelledby="stg-tab-this-install"
          data-stg="this-install"
          hidden={!!query || active !== 'this-install'}
        >
          <header className="stg-head">
            <h2 className="stg-title">This install</h2>
            <p className="stg-lead">The build you're running and the first-run steps.</p>
          </header>
          <div className="pn-list pn-list--tight">
            <div className="pn-row" data-stg="install-version">
              <div>
                <div className="pn-name">OpenRemote {version}</div>
                <div className="pn-detail">Daemon {capabilities?.daemon ?? '-'} · the parity build</div>
              </div>
              <button type="button" className="pn-btn" onClick={() => setAbout(true)}>About</button>
            </div>
            {!isMobile && (
              <div className="pn-row" data-stg="install-first-run">
                <div>
                  <div className="pn-name">First-run steps</div>
                  <div className="pn-detail">Shows them again. The chats stay.</div>
                </div>
                <button type="button" className="pn-btn" onClick={onReplay}>Show</button>
              </div>
            )}
          </div>
        </section>

      </div>
      )}
      {deleting && (
        <DeleteChatModal
          title={chats[deleting.id]?.title ?? deleting.title ?? 'This chat'}
          onClose={() => setDeleting(null)}
          onDelete={() => deleteChat(deleting.id)}
        />
      )}
      {about && (
        <AboutModal
          onClose={() => setAbout(false)}
          daemonVersion={capabilities?.daemon ?? null}
          api={api}
        />
      )}
      {signInFor && <SignInModal harnessId={signInFor} onDone={() => setSignInFor(null)} />}
      {managingEndpoints && defaultHarness && (
        <EndpointManagerModal
          harness={defaultHarness.id}
          deviceId={null}
          where="this computer"
          onClose={() => setManagingEndpoints(false)}
        />
      )}
    </div>
  )
}

import { useEffect, useState } from 'react'
import Sidebar from './Sidebar.jsx'
import TitleBar from './TitleBar.jsx'
import NewChat from './NewChat.jsx'
import ChatView from './ChatView.jsx'
import CloudMode from './CloudMode.jsx'
import Onboarding from './Onboarding.jsx'
import SettingsModal from './SettingsModal.jsx'
import PluginsView from './PluginsView.jsx'
import AutomationsView from './AutomationsView.jsx'
import MachinesView from './MachinesView.jsx'
import { ConsoleProvider, useConsole } from './state/console.jsx'

const OB_KEY = 'openremote-onboarded'
const STORE_KEY = 'openremote-view-state'
// 'machines' and 'settings' are deliberately absent - machines returns
// with remote check-in, settings is now a popup, not a route; a stale
// hash or stored history referencing either falls back to New chat.
const STATIC_VIEWS = ['new', 'plugins', 'automations']

function loadViewState(sessions) {
  let saved = null
  try {
    saved = JSON.parse(sessionStorage.getItem(STORE_KEY) ?? 'null')
  } catch {
    saved = null
  }
  const known = new Set([...STATIC_VIEWS, ...sessions.map((s) => s.id)])
  const hashView = location.hash.replace(/^#/, '')
  const fromHash = known.has(hashView) ? hashView : null
  let history
  let hIndex
  if (saved?.history?.some((v) => known.has(v))) {
    history = saved.history.filter((v) => known.has(v))
    hIndex = Math.min(saved.hIndex ?? 0, history.length - 1)
  } else {
    history = ['new']
    hIndex = 0
  }
  if (fromHash && history[hIndex] !== fromHash) {
    history = [...history.slice(0, hIndex + 1), fromHash]
    hIndex = history.length - 1
  }
  return { history, hIndex }
}

function Shell() {
  const { sessions, chats, connection, ensureChat } = useConsole()
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
  const [{ history, hIndex }, setNav] = useState(() => loadViewState([]))
  const [onboarded, setOnboarded] = useState(() => {
    try {
      return localStorage.getItem(OB_KEY) === '1'
    } catch {
      return true
    }
  })
  // Settings is a popup over whatever view is open - not a route.
  const [settingsOpen, setSettingsOpen] = useState(false)
  // Local vs Cloud - Cloud swaps the whole main area for its coming-soon
  // state; the session history stays untouched underneath, so switching
  // back returns to the exact view. Within cloud, Machines is a real
  // view of its own ('hero' is the coming-soon pane).
  const [mode, setMode] = useState('local')
  const [cloudView, setCloudView] = useState('hero')

  const view = history[hIndex]
  const chat = STATIC_VIEWS.includes(view) ? null : chats[view] ?? null

  // Reconcile navigation with the live session list (a chat in the history
  // that no longer exists falls back to New chat). A chat reached by hash
  // still needs its record - ensure it, the same as a sidebar open.
  useEffect(() => {
    const state = loadViewState(sessions)
    setNav(state)
    const view = state.history[state.hIndex]
    const session = (sessions ?? []).find((s) => s.id === view)
    if (session) ensureChat(session)
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
    window.history.replaceState(null, '', `#${view}`)
  }, [view])

  const navigate = (id) => {
    if (id === view) return
    setNav((nav) => ({
      history: [...nav.history.slice(0, nav.hIndex + 1), id],
      hIndex: nav.hIndex + 1,
    }))
  }

  const goBack = () => setNav((nav) => ({ ...nav, hIndex: Math.max(0, nav.hIndex - 1) }))
  const goForward = () =>
    setNav((nav) => ({ ...nav, hIndex: Math.min(nav.history.length - 1, nav.hIndex + 1) }))

  // Opening a chat from the sidebar: make sure its record exists even
  // before the next session poll.
  const openSession = (id) => {
    const session = (sessions ?? []).find((s) => s.id === id)
    if (session) ensureChat(session)
    navigate(id)
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

  const body = () => (
    <>
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
      />
      {!onboarded ? (
        <div className="app-body">
          <Onboarding onDone={finishOnboarding} />
        </div>
      ) : (
        <div className="app-body">
          {sidebarOpen && <div className="sb-backdrop" onClick={() => setSidebarOpen(false)} />}
          <Sidebar
            open={sidebarOpen}
            active={view === 'new' ? null : view}
            mode={mode}
            cloudView={cloudView}
            onCloudView={setCloudView}
            onMode={(m) => {
              setMode(m)
              setCloudView('hero')
            }}
            onSelect={openSession}
            onOpenSettings={() => setSettingsOpen(true)}
          />
          <main className="main">
            {mode === 'cloud' ? (
              cloudView === 'machines' ? (
                <MachinesView onOpenChat={openSession} />
              ) : (
                <CloudMode onBackToLocal={() => setMode('local')} />
              )
            ) : (
              <>
                {view === 'new' && <NewChat onOpen={openSession} />}
            {view === 'plugins' && <PluginsView />}
            {view === 'automations' && <AutomationsView onOpenChat={openSession} />}
                {!STATIC_VIEWS.includes(view) &&
                  (chat ? (
                    <ChatView key={chat.id} chat={chat} />
                  ) : (
                    <div className="cv-none">That chat is no longer in this workspace.</div>
                  ))}
              </>
            )}
            {settingsOpen && (
              <SettingsModal onReplay={replayOnboarding} onClose={() => setSettingsOpen(false)} />
            )}
          </main>
        </div>
      )}
    </>
  )

  return body()
}

export default function App() {
  return (
    <ConsoleProvider>
      <div className="app">
        <Shell />
      </div>
    </ConsoleProvider>
  )
}

import { useEffect, useState } from 'react'
import Sidebar from './Sidebar.jsx'
import TitleBar from './TitleBar.jsx'
import NewChat from './NewChat.jsx'
import ChatView from './ChatView.jsx'
import CloudMode from './CloudMode.jsx'
import Onboarding from './Onboarding.jsx'
import SettingsView from './SettingsView.jsx'
import PluginsView from './PluginsView.jsx'
import AutomationsView from './AutomationsView.jsx'
import MachinesView from './MachinesView.jsx'
import { ConsoleProvider, useConsole } from './state/console.jsx'
import { needsSignIn } from './cloud.js'

const OB_KEY = 'openremote-onboarded'
const STORE_KEY = 'openremote-view-state'
const MODE_KEY = 'openremote-mode'
const STATIC_VIEWS = ['new', 'plugins', 'automations', 'machines', 'settings']

// The mode the user left the app in. Local unless they chose Cloud, so a
// first launch never asks anyone to sign in.
function loadMode() {
  try {
    return localStorage.getItem(MODE_KEY) === 'cloud' ? 'cloud' : 'local'
  } catch {
    return 'local'
  }
}

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
  const { sessions, chats, connection, ensureChat, cloud } = useConsole()
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
  // Settings is a view like the others - the gear navigates to it.
  // Local vs Cloud: Local is this computer's chats; Cloud is the synced
  // chats of every device, behind a moreweb sign-in the first time.
  const [mode, setMode] = useState(loadMode)

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
  }, [view, onboarded])

  const navigate = (id) => {
    if (id === view) return
    setNav((nav) => ({
      history: [...nav.history.slice(0, nav.hIndex + 1), id],
      hIndex: nav.hIndex + 1,
    }))
  }

  const switchMode = (next) => {
    setMode(next)
    try {
      localStorage.setItem(MODE_KEY, next)
    } catch {
      /* storage unavailable */
    }
    navigate('new')
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
            onMode={switchMode}
            onSelect={openSession}
          />
          <main className="main">
            {view === 'settings' ? (
              <SettingsView onReplay={replayOnboarding} />
            ) : mode === 'cloud' && needsSignIn(cloud) ? (
              <CloudMode onBackToLocal={() => switchMode('local')} />
            ) : (
              <>
                {view === 'new' && <NewChat onOpen={openSession} mode={mode} />}
                {view === 'machines' && <MachinesView onOpenChat={openSession} />}
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

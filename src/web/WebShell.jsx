// The web console's shell: the desktop console's Cloud mode in a browser.
// The sidebar, New chat and the chat view are the desktop's own components;
// only the state behind them is different, and it comes from the peer link.

import { useEffect, useState } from 'react'
import Sidebar from '../Sidebar.jsx'
import NewChat from '../NewChat.jsx'
import ChatView from '../ChatView.jsx'
import { KeyboardProvider } from '../keyboard.jsx'
import { useConsole } from '../state/console.jsx'
import { WebConsoleProvider } from './console.jsx'
import LoadingStatus from './LoadingStatus.jsx'
import WebTitleBar from './WebTitleBar.jsx'
import WebMachinesView from './WebMachinesView.jsx'
import WebAccountView from './WebAccountView.jsx'
import WebSettingsView from './WebSettingsView.jsx'
import './web-shell.css'

const STATIC = new Set(['new', 'machines', 'account', 'settings'])

function Shell({ onSignOut }) {
  const {
    sessions,
    chats,
    ensureChat,
    watchChats,
    machineId,
    setMachineId,
    machineLoading,
    machineError,
    retryMachine,
    chatLoads,
    sessionsAnswered,
  } = useConsole()
  const [view, setView] = useState('new')
  const [history, setHistory] = useState(['new'])
  const [index, setIndex] = useState(0)
  const [sidebarOpen, setSidebarOpen] = useState(() =>
    window.matchMedia('(min-width: 640px)').matches,
  )
  const [narrow, setNarrow] = useState(() => !window.matchMedia('(min-width: 640px)').matches)

  useEffect(() => {
    const media = window.matchMedia('(min-width: 640px)')
    const on = (event) => {
      setNarrow(!event.matches)
      setSidebarOpen(event.matches)
    }
    media.addEventListener('change', on)
    return () => media.removeEventListener('change', on)
  }, [])

  useEffect(() => {
    watchChats(STATIC.has(view) ? [] : [view])
  }, [view, watchChats])

  const go = (next) => {
    if (!next || next === view) return
    setHistory((current) => [...current.slice(0, index + 1), next])
    setIndex((current) => current + 1)
    setView(next)
    if (narrow) setSidebarOpen(false)
  }

  const open = (id) => {
    const session = sessions.find((row) => row.id === id)
    if (session) {
      ensureChat(session)
      if (session.executor) {
        setMachineId(session.executor)
      }
    }
    go(id)
  }

  // The sidebar names static views and chats with the same callback: a chat
  // must pass through open, which ensures its record, before the view changes.
  const select = (next) => {
    if (STATIC.has(next)) go(next)
    else open(next)
  }

  const back = () => {
    if (index <= 0) return
    setIndex(index - 1)
    setView(history[index - 1])
  }
  const forward = () => {
    if (index >= history.length - 1) return
    setIndex(index + 1)
    setView(history[index + 1])
  }

  return (
    <div className="web-shell">
      <WebTitleBar
        sidebarOpen={sidebarOpen}
        onToggleSidebar={() => setSidebarOpen((current) => !current)}
        onBack={back}
        onForward={forward}
        canBack={index > 0}
        canForward={index < history.length - 1}
      />
      <div className="app-body">
        {sidebarOpen && (
          <Sidebar
            open
            active={view === 'new' ? null : view}
            mode="cloud"
            web
            onMode={() => {}}
            onSelect={select}
            onSplit={null}
            onAddProject={null}
            onNewChatIn={() => go('new')}
            onReveal={() => setSidebarOpen(true)}
            onBrowser={null}
            onRenameChat={null}
            renamedTitles={{}}
          />
        )}
        {narrow && sidebarOpen && (
          <div className="sb-backdrop" onClick={() => setSidebarOpen(false)} />
        )}
        <main className="main">
          {!sessionsAnswered && <LoadingStatus>Connecting to your chats...</LoadingStatus>}
          {view === 'new' && machineLoading && <LoadingStatus>Loading your computer's apps...</LoadingStatus>}
          {view === 'new' && !machineLoading && machineError && (
            <div className="web-loading-error"><p>{machineError}</p><button type="button" onClick={retryMachine}>Try again</button></div>
          )}
          {view === 'new' && !machineLoading && !machineError && (
            <NewChat
              mode="cloud"
              web
              machinesOnly
              defaultTarget={machineId}
              onOpen={open}
              topRight={null}
            />
          )}
          {view === 'machines' && (
            <WebMachinesView
              onPick={(id) => {
                setMachineId(id)
                go('new')
              }}
            />
          )}
          {view === 'account' && <WebAccountView onSignOut={onSignOut} />}
          {view === 'settings' && <WebSettingsView />}
          {!STATIC.has(view) &&
            (chats[view] && chatLoads[view] === 'ready' ? (
              <ChatView chat={chats[view]} web onBack={() => go('new')} />
            ) : (
              <LoadingStatus>{chatLoads[view] === 'error' ? 'The chat could not be reached. Retrying...' : 'Loading the chat...'}</LoadingStatus>
            ))}
        </main>
      </div>
    </div>
  )
}

export default function WebShell({ machine, peer, tokens, target, onSignOut }) {
  return (
    <WebConsoleProvider
      peer={peer}
      tokens={tokens}
      target={target}
      initialMachineId={machine?.id ?? null}
      onSignOut={onSignOut}
    >
      <KeyboardProvider>
        <Shell onSignOut={onSignOut} />
      </KeyboardProvider>
    </WebConsoleProvider>
  )
}

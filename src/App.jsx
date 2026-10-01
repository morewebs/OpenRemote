import { useEffect, useState } from 'react'
import Sidebar from './Sidebar.jsx'
import TitleBar from './TitleBar.jsx'
import NewChat from './NewChat.jsx'
import DevicesView from './DevicesView.jsx'
import ChatView from './ChatView.jsx'
import Onboarding from './Onboarding.jsx'
import Panels from './Panels.jsx'
import PluginsView from './PluginsView.jsx'
import AutomationsView from './AutomationsView.jsx'
import { APP, HARNESSES, MODELS, harnessName, modelName } from './catalog.js'
import {
  WORLD_KEY,
  acknowledgePluginKey,
  addAutomation,
  addCustomPlugin,
  addDevice,
  addStarter,
  installFromCatalog,
  installOnDevice,
  loadWorld,
  markUpdated,
  openChat,
  resetWorld,
  removeAutomation,
  removePlugin,
  resolveTool,
  runAutomation,
  seedWorld,
  sendMessage,
  setChatHarness,
  setChatModel,
  setDefaults,
  setDeviceOnline,
  setMode,
  setPluginEnabled,
  setReduceMotion,
  updateAutomation,
  signInHarness,
  signOutHarness,
  toggleAutomation,
} from './world.js'

const OB_KEY = 'openremote-ui-onboarded'
const STORE_KEY = 'openremote-ui-state'
const STATIC_VIEWS = ['new', 'devices', 'plugins', 'automations', 'settings']

function loadInitial() {
  let world
  try {
    world = loadWorld(localStorage.getItem(WORLD_KEY))
  } catch {
    world = loadWorld(null)
  }
  let saved = null
  try {
    saved = JSON.parse(sessionStorage.getItem(STORE_KEY) ?? 'null')
  } catch {
    saved = null
  }
  const known = new Set([...STATIC_VIEWS, ...world.chats.map((c) => c.id)])
  const hashView = location.hash.replace(/^#/, '')
  const fromHash = known.has(hashView) ? hashView : null
  let history
  let hIndex
  if (saved?.history?.some((v) => known.has(v))) {
    history = saved.history.filter((v) => known.has(v))
    hIndex = Math.min(saved.hIndex ?? 0, history.length - 1)
  } else {
    history = [fromHash ?? 'new']
    hIndex = 0
  }
  if (fromHash && history[hIndex] !== fromHash) {
    history = [...history.slice(0, hIndex + 1), fromHash]
    hIndex = history.length - 1
  }
  return { world, history, hIndex }
}

const restored = loadInitial()

function visibleHarnesses(signedIn) {
  return HARNESSES.map((h) => ({
    ...h,
    group: signedIn.includes(h.id) ? 'Signed in' : 'Available',
  })).sort((a, b) => (a.group === b.group ? 0 : a.group === 'Signed in' ? -1 : 1))
}

export default function App() {
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const [history, setHistory] = useState(restored.history)
  const [hIndex, setHIndex] = useState(restored.hIndex)
  const [world, setWorld] = useState(restored.world)
  const [onboarded, setOnboarded] = useState(() => {
    try {
      return localStorage.getItem(OB_KEY) === '1'
    } catch {
      return true
    }
  })
  const view = history[hIndex]
  const stale = world.schema !== 2 || !Array.isArray(world.plugins) || !Array.isArray(world.signedIn)
  if (stale) setWorld(seedWorld())
  const harnesses = visibleHarnesses(world.signedIn ?? [])

  const finishOnboarding = () => {
    try {
      localStorage.setItem(OB_KEY, '1')
    } catch {
      /* storage unavailable */
    }
    setOnboarded(true)
  }

  useEffect(() => {
    try {
      localStorage.setItem(WORLD_KEY, JSON.stringify(world))
    } catch {
      /* storage unavailable */
    }
  }, [world])

  useEffect(() => {
    try {
      sessionStorage.setItem(STORE_KEY, JSON.stringify({ history, hIndex }))
    } catch {
      /* storage unavailable */
    }
  }, [history, hIndex])

  useEffect(() => {
    document.documentElement.toggleAttribute('data-reduce-motion', world.reduceMotion)
  }, [world.reduceMotion])

  useEffect(() => {
    window.history.replaceState(null, '', `#${view}`)
  }, [view])

  const navigate = (id) => {
    if (id === view) return
    setHistory([...history.slice(0, hIndex + 1), id])
    setHIndex((i) => i + 1)
  }

  const goBack = () => setHIndex((i) => Math.max(0, i - 1))
  const goForward = () => setHIndex((i) => Math.min(history.length - 1, i + 1))

  const switchHarness = (chatId, harnessId) => {
    const modelId = MODELS[harnessId]?.[0]?.id
    setWorld((w) => setChatHarness(w, chatId, harnessId, modelId, modelName(harnessId, modelId)))
  }

  const switchModel = (chatId, modelId) => {
    const chat = world.chats.find((c) => c.id === chatId)
    setWorld((w) => setChatModel(w, chatId, modelId, modelName(chat?.harness, modelId)))
  }

  const createChat = (text, harness, model, deviceId, projectId) => {
    const id = crypto.randomUUID()
    setWorld((w) => openChat(w, {
      id,
      text,
      harness,
      harnessName: harnessName(harness),
      model,
      modelLabel: modelName(harness, model),
      deviceId,
      mode: w.mode,
      project: projectId,
    }))
    navigate(id)
  }

  const runRule = (id) => {
    const ran = runAutomation(world, id)
    setWorld(ran.world)
    if (ran.chatId) navigate(ran.chatId)
  }

  const reset = () => {
    setWorld(resetWorld())
    setHistory(['new'])
    setHIndex(0)
  }

  const replay = () => {
    try {
      localStorage.removeItem(OB_KEY)
    } catch {
      /* storage unavailable */
    }
    setOnboarded(false)
  }

  const staticView = STATIC_VIEWS.includes(view)
  const chat = stale ? null : world.chats.find((c) => c.id === view)
  if (stale) return null

  return (
    <div className="app" data-reduce-motion={world.reduceMotion ? '' : undefined}>
      {!onboarded && (
        <Onboarding
          onDone={finishOnboarding}
          onSignIn={(id) => setWorld((w) => signInHarness(w, id))}
          harnesses={harnesses}
        />
      )}
      {onboarded && (
        <>
          <TitleBar
            sidebarOpen={sidebarOpen}
            onToggleSidebar={() => setSidebarOpen((v) => !v)}
            onBack={goBack}
            onForward={goForward}
            canBack={hIndex > 0}
            canForward={hIndex < history.length - 1}
            version={world.appVersion}
            latest={APP.latest}
            reduceMotion={world.reduceMotion}
            onUpdated={() => setWorld((w) => markUpdated(w, APP.latest))}
          />
          <div className="app-body">
            <Sidebar
              open={sidebarOpen}
              active={view === 'new' ? null : view}
              onSelect={navigate}
              mode={world.mode}
              onModeChange={(mode) => setWorld((w) => setMode(w, mode))}
              chats={world.chats}
            />
            <main className="main">
              {view === 'new' && (
                <NewChat
                  mode={world.mode}
                  devices={world.devices}
                  harnesses={harnesses}
                  defaults={world.defaults}
                  chats={world.chats}
                  onOpen={navigate}
                  onCreate={createChat}
                />
              )}
              {view === 'devices' && (
                <DevicesView
                  devices={world.devices}
                  signedIn={world.signedIn}
                  onOpenChat={navigate}
                  onAdd={(input) => setWorld((w) => addDevice(w, input))}
                  onTogglePower={(id, online) => setWorld((w) => setDeviceOnline(w, id, online))}
                  onInstall={(deviceId, harnessId) => setWorld((w) => installOnDevice(w, deviceId, harnessId))}
                />
              )}
              {view === 'plugins' && (
                <PluginsView
                  world={world}
                  onInstall={(catalogId, deviceId) => setWorld((w) => installFromCatalog(w, catalogId, deviceId))}
                  onAdd={(input) => setWorld((w) => addCustomPlugin(w, input))}
                  onRemove={(id) => setWorld((w) => removePlugin(w, id))}
                  onKey={(id) => setWorld((w) => acknowledgePluginKey(w, id))}
                  onEnabled={(id, enabled) => setWorld((w) => setPluginEnabled(w, id, enabled))}
                />
              )}
              {view === 'automations' && (
                <AutomationsView
                  world={world}
                  harnesses={harnesses}
                  onToggle={(id) => setWorld((w) => toggleAutomation(w, id))}
                  onRun={runRule}
                  onOpenChat={navigate}
                  onAdd={(input) => setWorld((w) => addAutomation(w, input))}
                  onUpdate={(id, input) => setWorld((w) => updateAutomation(w, id, input))}
                  onRemove={(id) => setWorld((w) => removeAutomation(w, id))}
                  onStarter={(id) => setWorld((w) => addStarter(w, id))}
                />
              )}
              {view === 'settings' && (
                <Panels
                  world={world}
                  harnesses={harnesses}
                  onDefaults={(patch) => setWorld((w) => setDefaults(w, patch))}
                  onSignIn={(id) => setWorld((w) => signInHarness(w, id))}
                  onSignOut={(id) => setWorld((w) => signOutHarness(w, id))}
                  onReduceMotion={(value) => setWorld((w) => setReduceMotion(w, value))}
                  onReset={reset}
                  onReplay={replay}
                  onUpdated={() => setWorld((w) => markUpdated(w, APP.latest))}
                />
              )}
              {!staticView && (
                chat ? (
                  <ChatView
                    key={chat.id}
                    chat={chat}
                    devices={world.devices}
                    harnesses={harnesses}
                    onSend={(id, text) => setWorld((w) => sendMessage(w, id, text))}
                    onResolve={(id, messageId, choice) => setWorld((w) => resolveTool(w, id, messageId, choice))}
                    onHarness={switchHarness}
                    onModel={switchModel}
                  />
                ) : (
                  <div className="cv-none">That session is no longer in this workspace.</div>
                )
              )}
            </main>
          </div>
        </>
      )}
    </div>
  )
}

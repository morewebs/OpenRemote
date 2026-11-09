// The web console's state, in the shape the desktop console's own views read.
//
// A browser has no daemon and no harnesses. Its only compute is a machine the
// account owns, reached through the encrypted link. This provider is the
// desktop ConsoleProvider's public surface, sourced from that link instead of
// a local daemon: the summary route for the chat list, the chat range route
// for the open transcript, and the machine-tier routes for everything a
// person can do from here. Routes the peer gate does not carry stay absent.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ConsoleContext, useConsole } from '../state/console.jsx'
import { chatFromSession, foldEvents } from '../state/reducer.js'
import { advanceCursor, mergeSyncedSummaries, syncedSession } from './sync-view.js'
import { webApi } from './webapi.js'
import { browserDeviceName } from './peer.js'

const NO_PLUGINS = { installed: [] }
const EMPTY = new Set()
const NOOP = async () => {}
const UNAVAILABLE = async () => {
  throw new Error('That action is not available from the web console.')
}

const machineList = (devices) =>
  (Array.isArray(devices) ? devices : []).filter((device) => device?.kind === 'machine')

export function WebConsoleProvider({
  peer,
  tokens,
  target,
  initialMachineId = null,
  onSignOut,
  children,
}) {
  const apiBase = target?.api
  const access = tokens?.access
  const [connection, setConnection] = useState({ state: 'connected' })
  const [devices, setDevices] = useState([])
  const [devicesAnswered, setDevicesAnswered] = useState(false)
  const [cloud, setCloud] = useState({ state: 'online', account: null, device: null })
  const [thisDevice, setThisDevice] = useState(null)
  const [sessions, setSessions] = useState([])
  const [sessionsAnswered, setSessionsAnswered] = useState(false)
  const [chats, setChats] = useState({})
  const [capabilities, setCapabilities] = useState(null)
  const [capabilitiesByDevice, setCapabilitiesByDevice] = useState({})
  const [modelsByHarness, setModelsByHarness] = useState({})
  const [endpointsByHarness, setEndpointsByHarness] = useState({})
  const [projects, setProjects] = useState([])
  const [projectsAnswered, setProjectsAnswered] = useState(false)
  const [machineId, setMachineId] = useState(initialMachineId)
  const [watched, setWatched] = useState([])

  const alive = useRef(true)
  const hydrated = useRef(new Map())
  const cursors = useRef(new Map())
  const summaryInFlight = useRef(false)
  const chatsRef = useRef(chats)
  chatsRef.current = chats
  const sessionsRef = useRef(sessions)
  sessionsRef.current = sessions
  const machineRef = useRef(machineId)
  machineRef.current = machineId
  const capsRef = useRef(capabilitiesByDevice)
  capsRef.current = capabilitiesByDevice

  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])

  const apiFor = useCallback(
    (deviceId) => webApi(peer, deviceId ?? machineRef.current),
    [peer],
  )

  // The browser takes its seat in the account once. The id is what the
  // desktop's `thisDevice` means: a device of the account, never a machine.
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const me = await peer.ensureDevice({
          api: apiBase,
          accessToken: access,
          name: browserDeviceName(),
        })
        if (cancelled || !alive.current) return
        setThisDevice(me?.id ?? null)
        setCloud((current) => ({
          ...current,
          device: { id: me?.id ?? null, name: me?.name ?? 'OpenRemote Web' },
        }))
      } catch (err) {
        if (!cancelled && alive.current) {
          setConnection({
            state: 'error',
            error: err?.message ?? 'The web link could not open.',
          })
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [peer, apiBase, access])

  // The account's devices come from the gateway, with the account tokens.
  // This is the one route the browser calls directly rather than through a
  // machine: it is what tells the page which machines exist.
  const refreshDevices = useCallback(async () => {
    if (!apiBase || !access) return
    try {
      const response = await fetch(`${apiBase}/devices`, {
        headers: { Authorization: `Bearer ${access}` },
      })
      if (!response.ok) throw new Error(`Your account answered ${response.status}.`)
      const payload = await response.json().catch(() => null)
      if (!alive.current) return
      const rows = Array.isArray(payload?.devices) ? payload.devices : []
      setDevices(rows)
      setDevicesAnswered(true)
      setCloud((current) => ({
        ...current,
        state: 'online',
        account: payload?.account ?? current.account,
      }))
      if (!machineRef.current) {
        const first = machineList(rows).find((device) => device.online) ?? machineList(rows)[0]
        if (first) setMachineId(first.id)
      }
    } catch (err) {
      if (!alive.current) return
      setConnection({
        state: 'error',
        error: err?.message ?? 'Your account could not be reached.',
      })
    }
  }, [apiBase, access])

  // The chat list: every online machine's summary, merged, with the first
  // rows' own logs read to recover the Session facts the summary omits.
  const refreshSummaries = useCallback(async () => {
    if (summaryInFlight.current) return
    summaryInFlight.current = true
    try {
      const machines = machineList(devices).filter((device) => device.online !== false)
      const payloads = await Promise.all(
        machines.map(async (machine) => {
          try {
            const payload = await apiFor(machine.id).call('GET', '/sync/summary', null, {
              timeoutMs: 15000,
            })
            return { machine: machine.id, payload }
          } catch {
            return null
          }
        }),
      )
      if (!alive.current) return
      const rows = mergeSyncedSummaries(payloads.filter(Boolean))
      const missing = rows.filter((row) => !hydrated.current.has(row.id)).slice(0, 12)
      let next = 0
      const worker = async () => {
        while (next < missing.length) {
          const row = missing[next]
          next += 1
          try {
            const page = await apiFor(row.executor).call(
              'GET',
              `/sync/sessions/${encodeURIComponent(row.id)}/events?from=0`,
              null,
              { timeoutMs: 20000 },
            )
            hydrated.current.set(row.id, syncedSession(row, page?.events ?? []))
          } catch {
            // A row whose log cannot be read still lists; opening it retries.
          }
        }
      }
      await Promise.all(Array.from({ length: Math.min(3, missing.length) }, worker))
      if (!alive.current) return
      setSessions(rows.map((row) => hydrated.current.get(row.id) ?? syncedSession(row, [])))
      setSessionsAnswered(true)
    } finally {
      summaryInFlight.current = false
    }
  }, [devices, apiFor])

  useEffect(() => {
    void refreshDevices()
  }, [refreshDevices])

  useEffect(() => {
    if (!devicesAnswered) return
    void refreshSummaries()
    const timer = setInterval(() => void refreshSummaries(), 5000)
    return () => clearInterval(timer)
  }, [devicesAnswered, refreshSummaries])

  // The open chat's transcript, folded from the range route on a short
  // timer. A peer cannot be handed the daemon's live SSE; this is the same
  // reducer over a polled page, which is what the chat view already reads.
  useEffect(() => {
    const timers = new Map()
    const tick = async (id) => {
      const chat = chatsRef.current[id]
      if (!chat?.executor) return
      try {
        const page = await apiFor(chat.executor).call(
          'GET',
          `/sync/sessions/${encodeURIComponent(id)}/events?from=${cursors.current.get(id) ?? 0}`,
          null,
          { timeoutMs: 20000 },
        )
        if (!alive.current) return
        const events = Array.isArray(page?.events) ? page.events : []
        if (!events.length) return
        const folded = foldEvents(
          { ...chat, timeline: [...chat.timeline] },
          events,
        )
        cursors.current.set(id, advanceCursor(cursors.current.get(id) ?? 0, page))
        setChats((current) => ({ ...current, [id]: folded }))
        if (folded.title) {
          setSessions((current) =>
            current.map((session) =>
              session.id === id ? { ...session, title: folded.title } : session,
            ),
          )
        }
      } catch {
        // The next tick retries; a deleted chat is removed by the summary.
      }
    }
    for (const id of watched) {
      void tick(id)
      timers.set(id, setInterval(() => void tick(id), 1000))
    }
    return () => {
      for (const timer of timers.values()) clearInterval(timer)
    }
  }, [watched, apiFor])

  // The selected machine's capabilities and projects. The capabilities are
  // also what ChatView reads for the open chat's harness, so opening a chat
  // on another machine moves the selection there first.
  const capabilitiesFor = useCallback(
    async (deviceId) => {
      const machine = deviceId ?? machineRef.current
      if (!machine) return null
      try {
        const caps = await apiFor(machine).capabilities()
        if (alive.current) {
          setCapabilitiesByDevice((current) => ({ ...current, [machine]: caps }))
        }
        return caps
      } catch {
        return capsRef.current[machine] ?? null
      }
    },
    [apiFor],
  )

  useEffect(() => {
    if (!machineId) return
    let cancelled = false
    capabilitiesFor(machineId)
      .then((caps) => {
        if (!cancelled && alive.current && caps) setCapabilities(caps)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [machineId, capabilitiesFor])

  useEffect(() => {
    if (!machineId) return
    let cancelled = false
    apiFor(machineId)
      .projects()
      .then((list) => {
        if (cancelled || !alive.current) return
        setProjects(Array.isArray(list) ? list : [])
        setProjectsAnswered(true)
      })
      .catch(() => {
        if (!cancelled && alive.current) setProjectsAnswered(true)
      })
    return () => {
      cancelled = true
    }
  }, [machineId, apiFor])

  const ensureChat = useCallback((session) => {
    if (!session?.id) return
    setChats((current) =>
      current[session.id] ? current : { ...current, [session.id]: chatFromSession(session) },
    )
  }, [])

  const watchChats = useCallback((ids) => {
    const next = [...new Set(Array.isArray(ids) ? ids : [])].sort()
    setWatched((current) =>
      current.length === next.length && current.every((id, index) => id === next[index])
        ? current
        : next,
    )
  }, [])

  const executorOf = useCallback(
    (chatId) =>
      chatsRef.current[chatId]?.executor ??
      sessionsRef.current.find((session) => session.id === chatId)?.executor ??
      null,
    [],
  )

  const createChat = useCallback(
    async (
      text,
      workspace,
      harness,
      model,
      fast,
      effort,
      permissionMode,
      { deviceId = null, mentionedChats = [], endpointId = null } = {},
    ) => {
      const machine = deviceId ?? machineRef.current
      const api = apiFor(machine)
      const session = await api.createSession({
        harness,
        workspace,
        model,
        effort,
        permissionMode,
        fast,
        endpointId,
        synced: true,
      })
      ensureChat(session)
      setSessions((current) => [session, ...current.filter((row) => row.id !== session.id)])
      if (text?.trim()) {
        api.prompt(session.id, text.trim(), mentionedChats).catch(() => {
          setChats((current) => {
            const chat = current[session.id]
            if (!chat) return current
            return {
              ...current,
              [session.id]: foldEvents(
                { ...chat, timeline: [...chat.timeline] },
                [
                  {
                    type: 'daemon.error',
                    id: `send-failed-${Date.now()}`,
                    seq: null,
                    message: 'The first message could not be sent.',
                  },
                ],
              ),
            }
          })
        })
      }
      return session.id
    },
    [apiFor, ensureChat],
  )

  const sendPrompt = useCallback(
    (chatId, text, mentioned = []) => {
      const executor = executorOf(chatId)
      return apiFor(executor).prompt(chatId, text, mentioned)
    },
    [apiFor, executorOf],
  )

  const updateChatSettings = useCallback(
    (chatId, settings) => {
      const executor = executorOf(chatId)
      return apiFor(executor).updateSettings(chatId, settings)
    },
    [apiFor, executorOf],
  )

  const answerDecision = useCallback(
    (decisionId, choice) => {
      const chat = Object.values(chatsRef.current).find(
        (row) =>
          row.pendingDecisionId === decisionId ||
          row.timeline.some((item) => item.kind === 'decision' && item.id === decisionId),
      )
      if (!chat?.executor) return Promise.reject(new Error('That decision is not in a reachable chat.'))
      return apiFor(chat.executor).answer(decisionId, choice)
    },
    [apiFor],
  )

  const chatAction = useCallback(
    (chatId, run) => {
      const executor = executorOf(chatId)
      return run(apiFor(executor))
    },
    [apiFor, executorOf],
  )

  const stopChat = useCallback((chatId) => chatAction(chatId, (api) => api.stop(chatId)), [chatAction])
  const interruptChat = useCallback(
    (chatId) => chatAction(chatId, (api) => api.interrupt(chatId)),
    [chatAction],
  )
  const resumeChat = useCallback((chatId) => chatAction(chatId, (api) => api.resume(chatId)), [chatAction])
  const queueMessage = useCallback(
    (chatId, text, mentioned = []) => chatAction(chatId, (api) => api.queue(chatId, text, mentioned)),
    [chatAction],
  )
  const removeQueued = useCallback(
    (chatId, itemId) => chatAction(chatId, (api) => api.queueRemove(chatId, itemId)),
    [chatAction],
  )
  const steerQueued = useCallback(
    (chatId, itemId) => chatAction(chatId, (api) => api.queueSteer(chatId, itemId)),
    [chatAction],
  )
  const steerMessage = useCallback(
    (chatId, text, mentioned = []) => chatAction(chatId, (api) => api.steer(chatId, text, mentioned)),
    [chatAction],
  )
  const resumeQueue = useCallback(
    (chatId) => chatAction(chatId, (api) => api.queueResume(chatId)),
    [chatAction],
  )
  const renameChat = useCallback(
    (chatId, title) => chatAction(chatId, (api) => api.rename(chatId, title)),
    [chatAction],
  )

  const modelsFor = useCallback(
    async (harnessId, deviceId = null) => {
      const key = `${deviceId ?? machineRef.current}:${harnessId}`
      if (modelsByHarness[key]?.length) return modelsByHarness[key]
      try {
        const list = (await apiFor(deviceId).models(harnessId)) ?? []
        if (list.length) setModelsByHarness((current) => ({ ...current, [key]: list }))
        return list
      } catch {
        return []
      }
    },
    [apiFor, modelsByHarness],
  )

  const endpointsFor = useCallback(
    async (harnessId, deviceId = null) => {
      const key = `${deviceId ?? machineRef.current}:${harnessId}`
      try {
        const list = (await apiFor(deviceId).modelEndpoints({ harness: harnessId })) ?? []
        setEndpointsByHarness((current) => ({ ...current, [key]: list }))
        return list
      } catch {
        return endpointsByHarness[key] ?? []
      }
    },
    [apiFor, endpointsByHarness],
  )

  const refreshEndpoints = useCallback(
    async (harnessId, deviceId = null) => {
      const key = `${deviceId ?? machineRef.current}:${harnessId}`
      try {
        const list = (await apiFor(deviceId).modelEndpoints({ harness: harnessId })) ?? []
        setEndpointsByHarness((current) => ({ ...current, [key]: list }))
        return list
      } catch {
        return endpointsByHarness[key] ?? []
      }
    },
    [apiFor, endpointsByHarness],
  )

  const endpointModelsFor = useCallback(
    async (endpointId, deviceId = null) => {
      if (!endpointId) return []
      try {
        return (await apiFor(deviceId).endpointModels(endpointId)) ?? []
      } catch {
        return []
      }
    },
    [apiFor],
  )

  const setChatEndpoint = useCallback(
    async (chatId, endpointId = null) => {
      const executor = executorOf(chatId)
      const session = await apiFor(executor).setSessionEndpoint(chatId, endpointId)
      ensureChat(session)
      return session
    },
    [apiFor, ensureChat, executorOf],
  )

  const commandsFor = useCallback(
    async ({ harness, workspace = null, session = null, deviceId = null }) =>
      apiFor(deviceId).commands({ harness, workspace, session }),
    [apiFor],
  )

  const filesFor = useCallback(
    async ({ workspace = null, session = null, deviceId = null, q = '', signal = null }) =>
      apiFor(deviceId).files({ workspace, session, q }, { signal }),
    [apiFor],
  )

  const addProject = useCallback(
    async (folders, { deviceId = null } = {}) => {
      const project = await apiFor(deviceId).createProject({ folders })
      try {
        const list = (await apiFor(deviceId).projects()) ?? []
        if (alive.current) setProjects(Array.isArray(list) ? list : [])
      } catch {
        /* the next selection reads it again */
      }
      return project
    },
    [apiFor],
  )

  const removeProject = useCallback(
    (projectId) => apiFor(null).removeProject(projectId),
    [apiFor],
  )

  const installHarness = useCallback(
    (machine, harnessId, options = {}) => apiFor(machine).installHarness(machine, harnessId, options),
    [apiFor],
  )

  const signIn = useCallback((harnessId) => apiFor(null).startSignIn(harnessId), [apiFor])
  const feedSignIn = useCallback(
    (harnessId, text) => apiFor(null).signInInput(harnessId, text),
    [apiFor],
  )
  const stopSignIn = useCallback((harnessId) => apiFor(null).stopSignIn(harnessId), [apiFor])

  const value = useMemo(
    () => ({
      api: { decisions: async () => [], ready: true, url: apiBase },
      connection,
      capabilities,
      sessions,
      machines: [],
      mcpServers: [],
      plugins: NO_PLUGINS,
      stagePlugin: UNAVAILABLE,
      installStagedPlugin: UNAVAILABLE,
      cancelStagedPlugin: UNAVAILABLE,
      setPluginEnabled: UNAVAILABLE,
      removePlugin: UNAVAILABLE,
      marketplaces: [],
      catalog: [],
      pluginsError: null,
      marketplacesError: null,
      refreshPlugins: NOOP,
      addMarketplace: UNAVAILABLE,
      refreshMarketplace: UNAVAILABLE,
      setMarketplaceEnabled: UNAVAILABLE,
      removeMarketplace: UNAVAILABLE,
      automations: [],
      projects,
      machinesAnswered: true,
      sessionsAnswered,
      mcpServersAnswered: true,
      automationsAnswered: true,
      projectsAnswered,
      devicesAnswered,
      chats,
      reconnecting: false,
      watchChats,
      connect: NOOP,
      disconnect: NOOP,
      ensureChat,
      createChat,
      modelsFor,
      endpointsFor,
      refreshEndpoints,
      endpointModelsFor,
      setChatEndpoint,
      cloud,
      devices,
      thisDevice,
      apiFor,
      commandsFor,
      filesFor,
      renameChat,
      capabilitiesFor,
      refreshCloud: refreshDevices,
      signInCloud: UNAVAILABLE,
      cancelCloudSignIn: NOOP,
      signOutCloud: onSignOut,
      setDeviceKind: UNAVAILABLE,
      removeDevice: UNAVAILABLE,
      createEnrollment: UNAVAILABLE,
      deleteChat: UNAVAILABLE,
      archived: EMPTY,
      setChatArchived: NOOP,
      syncChat: UNAVAILABLE,
      sendPrompt,
      updateChatSettings,
      answerDecision,
      stopChat,
      interruptChat,
      queueMessage,
      removeQueued,
      steerQueued,
      steerMessage,
      resumeQueue,
      resumeChat,
      createMachine: UNAVAILABLE,
      removeMachine: UNAVAILABLE,
      installHarness,
      signIn,
      feedSignIn,
      stopSignIn,
      addMcpServer: UNAVAILABLE,
      removeMcpServer: UNAVAILABLE,
      setMcpServerEnabled: UNAVAILABLE,
      acknowledgeMcpServerKey: UNAVAILABLE,
      saveAutomation: UNAVAILABLE,
      removeAutomation: UNAVAILABLE,
      enableAutomation: UNAVAILABLE,
      runAutomation: UNAVAILABLE,
      addProject,
      removeProject,
      daemon: null,
      retryDaemon: NOOP,
      machineId,
      setMachineId,
    }),
    [
      apiBase,
      connection,
      capabilities,
      sessions,
      projects,
      sessionsAnswered,
      projectsAnswered,
      devicesAnswered,
      chats,
      cloud,
      devices,
      thisDevice,
      watchChats,
      ensureChat,
      createChat,
      modelsFor,
      endpointsFor,
      refreshEndpoints,
      endpointModelsFor,
      setChatEndpoint,
      apiFor,
      commandsFor,
      filesFor,
      renameChat,
      capabilitiesFor,
      refreshDevices,
      onSignOut,
      sendPrompt,
      updateChatSettings,
      answerDecision,
      stopChat,
      interruptChat,
      queueMessage,
      removeQueued,
      steerQueued,
      steerMessage,
      resumeQueue,
      resumeChat,
      installHarness,
      signIn,
      feedSignIn,
      stopSignIn,
      addProject,
      removeProject,
      machineId,
    ],
  )

  return <ConsoleContext.Provider value={value}>{children}</ConsoleContext.Provider>
}

export const useWebConsole = useConsole

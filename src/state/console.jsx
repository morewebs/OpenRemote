// The console's connection and chat state: one daemon connection, a
// polled session list for the sidebar, and one live SSE subscription for
// the open chat (resuming from its cursor across reconnects).

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react'
import { DaemonApi, clearConnection, loadConnection, saveConnection } from '../api/daemon.js'
import { chatFromSession, foldEvent } from './reducer.js'

const ConsoleContext = createContext(null)

const hasTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

export function ConsoleProvider({ children }) {
  const [api, setApi] = useState(() => {
    const saved = loadConnection()
    return new DaemonApi(saved.url, saved.token)
  })
  const [connection, setConnection] = useState(() => {
    const saved = loadConnection()
    return { state: saved.url && saved.token ? 'connecting' : 'new', error: null }
  })
  const [capabilities, setCapabilities] = useState(null)
  const [sessions, setSessions] = useState([])
  const [machines, setMachines] = useState([])
  const [plugins, setPlugins] = useState([])
  const [marketplace, setMarketplace] = useState([])
  const [automations, setAutomations] = useState([])
  const [chats, setChats] = useState({})
  const [modelsByHarness, setModelsByHarness] = useState({})
  // Cloud: this computer's account state, and the account's devices.
  const [cloud, setCloud] = useState(null)
  const [devices, setDevices] = useState([])
  const [capabilitiesByDevice, setCapabilitiesByDevice] = useState({})
  const [openChatId, setOpenChatId] = useState(null)
  const stopEventsRef = useRef(null)
  const cursorRef = useRef(null)
  const openChatRef = useRef(null)
  openChatRef.current = openChatId

  const connect = useCallback((url, token) => {
    saveConnection(url, token)
    setApi(new DaemonApi(url, token))
    setConnection({ state: 'connecting', error: null })
  }, [])

  const disconnect = useCallback(() => {
    clearConnection()
    stopEventsRef.current?.()
    stopEventsRef.current = null
    setApi(new DaemonApi(null, null))
    setConnection({ state: 'new', error: null })
    setSessions([])
    setChats({})
    setOpenChatId(null)
  }, [])

  // Fresh capabilities - the harness rows carry the harness's own status
  // words; a sign-in that settled re-reads them.
  const refreshCapabilities = useCallback(async () => {
    try {
      setCapabilities((await api.capabilities()) ?? null)
    } catch {
      /* the poll heals */
    }
  }, [api])

  // In the desktop shell the daemon is a sidecar: ask the shell for its
  // address once on boot before falling back to the saved connection.
  useEffect(() => {
    if (!hasTauri) return
    let cancelled = false
    let attempt = 0
    const ask = async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const info = await invoke('daemon_info')
        if (cancelled) return
        if (info?.url && info?.token) {
          connect(info.url, info.token)
        } else if (attempt < 150) {
          // The sidecar is still booting (a freshly built exe can take a
          // slow first run) - keep asking for a long while yet.
          attempt += 1
          setTimeout(ask, 600)
        } else if (!api.ready) {
          setConnection({ state: 'waiting-daemon', error: null })
        }
      } catch {
        /* the shell will report when it can; the wizard remains reachable */
      }
    }
    ask()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The desktop shell owns the sidecar: while the connection holds an
  // error, keep asking the shell for the daemon's address - a sidecar
  // that came back on a new port is followed without a reload. The
  // browser has only the address it was given; its error copy says so.
  useEffect(() => {
    if (!hasTauri || connection.state !== 'error') return
    let cancelled = false
    const ask = async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const info = await invoke('daemon_info')
        if (cancelled) return
        if (info?.url && info?.token) connect(info.url, info.token)
      } catch {
        /* still gone - the next tick asks again */
      }
    }
    const retry = setInterval(ask, 2500)
    return () => {
      cancelled = true
      clearInterval(retry)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connection.state])

  // Verify the connection, then load capabilities + sessions.
  useEffect(() => {
    if (!api.ready || connection.state === 'connected') return
    let cancelled = false
    ;(async () => {
    try {
      await api.health()
      const caps = await api.capabilities()
      const list = await api.sessions()
      const machineList = await api.machines()
      const pluginList = await api.plugins()
      const market = await api.pluginMarketplace()
      const rules = await api.automations()
      // Cloud is optional: a daemon without it still connects.
      const [cloudView, deviceList] = await Promise.allSettled([api.cloud(), api.cloudDevices()])
      if (cancelled) return
      if (cloudView.status === 'fulfilled') setCloud(cloudView.value)
      if (deviceList.status === 'fulfilled') setDevices(deviceList.value?.devices ?? [])
      setCapabilities(caps)
      setSessions(list ?? [])
      setMachines(machineList ?? [])
      setPlugins(pluginList ?? [])
      setMarketplace(market ?? [])
      setAutomations(rules ?? [])
      setConnection({ state: 'connected', error: null })
    } catch (err) {
        if (cancelled) return
        // A fetch that never answered is the daemon being unreachable -
        // say that, not the browser's own "Failed to fetch". Anything the
        // daemon answered with (a refused token, a status) is its words.
        const unreachable = err instanceof TypeError
        setConnection({
          state: 'error',
          error: unreachable
            ? "The daemon didn't answer - check that it's running, or reconnect from Settings."
            : err.message ?? String(err),
        })
      }
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, connection.state])

  // Poll the live lists while a daemon is configured (the sidebar's
  // truth; machines, plugins, and rules ride the same beat). A single
  // dropped call is nothing - but three beats with no answer at all
  // means the daemon is gone, and the connection says so instead of
  // freezing quietly on stale data. The poll keeps watching: the first
  // answering beat heals it, and no screen has to be reloaded.
  useEffect(() => {
    if (!api.ready) return
    let misses = 0
    const poll = setInterval(async () => {
      const [sessionList, machineList, pluginList, ruleList, cloudView, deviceList] =
        await Promise.allSettled([
          api.sessions(),
          api.machines(),
          api.plugins(),
          api.automations(),
          api.cloud(),
          api.cloudDevices(),
        ])
      if (cloudView.status === 'fulfilled') setCloud(cloudView.value)
      if (deviceList.status === 'fulfilled') setDevices(deviceList.value?.devices ?? [])
      if (sessionList.status === 'fulfilled') setSessions(sessionList.value ?? [])
      if (machineList.status === 'fulfilled') setMachines(machineList.value ?? [])
      if (pluginList.status === 'fulfilled') setPlugins(pluginList.value ?? [])
      if (ruleList.status === 'fulfilled') setAutomations(ruleList.value ?? [])
      const answered = [sessionList, machineList, pluginList, ruleList].some(
        (r) => r.status === 'fulfilled',
      )
      if (answered) {
        misses = 0
        if (connection.state === 'error') setConnection({ state: 'connected', error: null })
      } else {
        misses += 1
        if (misses >= 3 && connection.state === 'connected') {
          setConnection({
            state: 'error',
            error: 'The daemon stopped answering - it may have exited. Restart the app, or reconnect from Settings.',
          })
        }
      }
    }, 2500)
    return () => clearInterval(poll)
  }, [api, connection.state])

  // Keep chat records in step with the polled session list: merge
  // identities (status/workspace) without disturbing folded transcripts.
  // Event-folded facts the session payload doesn't carry (the context
  // numbers, the decisions list) survive the merge - the stream owns them.
  useEffect(() => {
    setChats((current) => {
      const next = { ...current }
      for (const session of sessions) {
        const existing = next[session.id]
        if (!existing) continue
        const fresh = chatFromSession(session)
        next[session.id] = {
          ...fresh,
          title: existing.title ?? fresh.title,
          timeline: existing.timeline,
          foldedSeq: existing.foldedSeq,
          pendingDecisionId: existing.pendingDecisionId,
          context: existing.context,
          decisionsList: existing.decisionsList,
        }
      }
      return next
    })
  }, [sessions])

  // The open chat: load its history + decisions, then stream live.
  const openChat = chats[openChatId] ?? null
  useEffect(() => {
    stopEventsRef.current?.()
    stopEventsRef.current = null
    if (!openChatId || connection.state !== 'connected') return
    let cancelled = false
    cursorRef.current = null
    ;(async () => {
      try {
        const list = await api.decisions(openChatId)
        if (cancelled) return
        setChats((current) => {
          const chat = current[openChatId]
          if (!chat) return current
          return { ...current, [openChatId]: { ...chat, decisionsList: list ?? [] } }
        })
      } catch {
        /* decisions load is best-effort; the stream carries them anyway */
      }
    })()

    const apply = (event) => {
      cursorRef.current = event.seq
      setChats((current) => {
        const chat = current[openChatId]
        if (!chat) return current
        const folded = foldEvent({ ...chat, timeline: [...chat.timeline] }, event)
        return { ...current, [openChatId]: folded }
      })
    }
    // Replay only what this chat hasn't folded yet - a chat reopened
    // after a switch already holds its timeline; `after: null` would
    // fold the whole history in a second time.
    const after = chats[openChatId]?.foldedSeq ?? null
    const stop = api.events(openChatId, {
      after,
      onEvent: apply,
      onClose: () => {
        if (cancelled || openChatRef.current !== openChatId) return
        // Reconnect from the cursor; the daemon replays anything missed.
        const retry = setTimeout(() => {
          if (cancelled || openChatRef.current !== openChatId) return
          stopEventsRef.current?.()
          stopEventsRef.current = api.events(openChatId, {
            after: cursorRef.current,
            onEvent: apply,
            onClose: () => {},
          })
        }, 800)
        stopEventsRef.current = () => {
          clearTimeout(retry)
        }
      },
    })
    stopEventsRef.current = stop
    return () => {
      cancelled = true
      stop()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [openChatId, connection.state])

  const ensureChat = useCallback(
    (session) => {
      // Merge into the map - the record never replaces it. Idempotent, so
      // the shell can ensure the chat for a hash-reached view too.
      setChats((current) =>
        current[session.id]
          ? current
          : { ...current, [session.id]: chatFromSession(session) },
      )
      setOpenChatId(session.id)
    },
    [],
  )

  // A new chat. In Cloud it syncs to every device, and with `deviceId` it
  // runs on that machine (created there, copied here); the prompt that
  // follows reaches it through this daemon either way.
  const createChat = useCallback(
    async (text, workspace, harness, model, fast, effort, { deviceId = null, synced = false } = {}) => {
      const session = await api.createSession({ harness, workspace, model, effort, fast, deviceId, synced })
      ensureChat(session)
      if (text.trim()) await api.prompt(session.id, text.trim())
      return session.id
    },
    [api, ensureChat],
  )

  // The API of another of the user's devices, through this daemon.
  const apiFor = useCallback((deviceId) => api.device(deviceId), [api])

  // The model catalog for a harness, cached per connection and device.
  // Empty means the harness advertises nothing - the picker stays hidden.
  const modelsFor = useCallback(
    async (harnessId, deviceId = null) => {
      const key = `${deviceId ?? ''}:${harnessId}`
      if (!api.ready || modelsByHarness[key]) return modelsByHarness[key] ?? []
      try {
        const list = (await api.device(deviceId).models(harnessId)) ?? []
        setModelsByHarness((current) => ({ ...current, [key]: list }))
        return list
      } catch {
        return []
      }
    },
    [api, modelsByHarness],
  )

  // What another device can run, fetched when a new chat aims at it.
  const capabilitiesFor = useCallback(
    async (deviceId) => {
      if (!deviceId) return capabilities
      try {
        const caps = await api.device(deviceId).capabilities()
        setCapabilitiesByDevice((current) => ({ ...current, [deviceId]: caps }))
        return caps
      } catch {
        return capabilitiesByDevice[deviceId] ?? null
      }
    },
    [api, capabilities, capabilitiesByDevice],
  )

  const refreshCloud = useCallback(async () => {
    const [view, list] = await Promise.allSettled([api.cloud(), api.cloudDevices()])
    if (view.status === 'fulfilled') setCloud(view.value)
    if (list.status === 'fulfilled') setDevices(list.value?.devices ?? [])
  }, [api])

  // Sign-in happens in the system browser, which the daemon opens; the
  // poll picks up the result when the browser comes back.
  const signInCloud = useCallback(async () => {
    const started = await api.cloudSignIn({ open: true })
    await refreshCloud()
    return started
  }, [api, refreshCloud])

  const cancelCloudSignIn = useCallback(async () => {
    setCloud(await api.cloudSignInCancel())
  }, [api])

  const signOutCloud = useCallback(async () => {
    setCloud(await api.cloudSignOut())
    setDevices([])
  }, [api])

  const setDeviceKind = useCallback(
    async (id, kind) => {
      await api.setDeviceKind(id, kind)
      // A change made from here isn't news here (the notices skip it).
      if (id === cloud?.device?.id) {
        try {
          localStorage.setItem(`openremote-kind-set-here:${id}`, JSON.stringify(kind))
        } catch {
          /* storage unavailable */
        }
      }
      await refreshCloud()
    },
    [api, refreshCloud, cloud],
  )

  const removeDevice = useCallback(
    async (id) => {
      await api.removeDevice(id)
      await refreshCloud()
    },
    [api, refreshCloud],
  )

  const createEnrollment = useCallback(() => api.createEnrollment(), [api])

  // The shell keeps the app in the tray by default when this computer is a
  // machine, so it hears when that changes.
  const thisKind = devices.find((d) => d.id === cloud?.device?.id)?.kind ?? null
  useEffect(() => {
    if (!hasTauri || thisKind == null) return
    ;(async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        await invoke('set_tray_prefs', { machine: thisKind === 'machine' })
      } catch {
        /* an older shell without a tray */
      }
    })()
  }, [thisKind])

  // A private chat joins Cloud: every device copies it, transcript and
  // tool output included; it keeps running here.
  const syncChat = useCallback(
    async (id) => {
      const session = await api.syncSession(id)
      setSessions((current) => current.map((s) => (s.id === id ? { ...s, ...session } : s)))
      setChats((current) =>
        current[id] ? { ...current, [id]: { ...current[id], executor: session.executor ?? null } } : current,
      )
    },
    [api],
  )

  // Deleting a synced chat removes it from every device.
  const deleteChat = useCallback(
    async (id) => {
      await api.deleteSession(id)
      setSessions((current) => current.filter((s) => s.id !== id))
      setChats((current) => {
        const next = { ...current }
        delete next[id]
        return next
      })
    },
    [api],
  )

  const sendPrompt = useCallback(
    async (chatId, text) => {
      await api.prompt(chatId, text.trim())
    },
    [api],
  )

  // Model, effort, or fast on a live chat. The session.updated event folds
  // the new facts; this returns the session so the composer can show a
  // harness that refused.
  const updateChatSettings = useCallback(
    async (chatId, settings) => api.updateSettings(chatId, settings),
    [api],
  )

  const answerDecision = useCallback(
    async (decisionId, choice) => {
      await api.answer(decisionId, choice)
    },
    [api],
  )

  const stopChat = useCallback(
    async (chatId) => {
      await api.stop(chatId)
    },
    [api],
  )

  // Interrupt the running turn - the agent stops what it's doing but the
  // session stays alive (stop kills the process; interrupt cancels the turn).
  const interruptChat = useCallback(
    async (chatId) => {
      await api.interrupt(chatId)
    },
    [api],
  )

  // ---- machines ----

  const refreshMachines = useCallback(async () => {
    try {
      setMachines((await api.machines()) ?? [])
    } catch {
      /* the poll heals */
    }
  }, [api])

  const createMachine = useCallback(
    async (name, platform) => {
      const machine = await api.createMachine({ name, platform })
      await refreshMachines()
      return machine
    },
    [api, refreshMachines],
  )

  const removeMachine = useCallback(
    async (machineId) => {
      await api.removeMachine(machineId)
      await refreshMachines()
    },
    [api, refreshMachines],
  )

  // The long one - an installer runs minutes. The receipt contract carries the
  // wait; a resolved promise means the harness landed on the machine.
  // Capabilities are re-read too: the picker, the settings list, and the
  // harness manager all read availability from there, not from machines.
  const installHarness = useCallback(
    async (machineId, harnessId, options) => {
      await api.installHarness(machineId, harnessId, options)
      await refreshMachines()
      await refreshCapabilities()
    },
    [api, refreshMachines, refreshCapabilities],
  )

  // ---- harness sign-in ----

  // The relay: the harness's own login command runs on the machine, the
  // CLI's words stream into the view, the human's answers ride back.
  // `onBeat` receives each view while the login runs; the promise settles
  // when the CLI exits, and the capabilities are re-read so the row
  // carries the harness's own fresh status words.
  // A harness's own sign-in, relayed - on this computer, or on one of the
  // account's machines (`deviceId`), where its CLI runs.
  const signIn = useCallback(
    async (harnessId, { onBeat, deviceId = null } = {}) => {
      const target = api.device(deviceId)
      let start
      try {
        start = await target.startSignIn(harnessId)
      } catch (err) {
        // A relay already running is one the human abandoned (or a modal
        // reopened): stop the stale attempt, then start fresh - the new
        // CLI's own login server is the one that must answer the browser.
        if (err?.status !== 409) throw err
        await target.stopSignIn(harnessId)
        await new Promise((r) => setTimeout(r, 400))
        start = await target.startSignIn(harnessId)
      }
      const first = start?.result ?? start
      onBeat?.(first)
      if (first?.done) {
        await refreshCapabilities()
        return first
      }
      return await new Promise((resolve) => {
        const poll = setInterval(async () => {
          let view = null
          try {
            view = await target.signInView(harnessId)
          } catch {
            return // the poll heals
          }
          onBeat?.(view)
          if (view?.done) {
            clearInterval(poll)
            await refreshCapabilities()
            resolve(view)
          }
        }, 800)
      })
    },
    [api, refreshCapabilities],
  )

  const feedSignIn = useCallback(
    async (harnessId, text, deviceId = null) => {
      await api.device(deviceId).signInInput(harnessId, text)
    },
    [api],
  )

  const stopSignIn = useCallback(
    async (harnessId, deviceId = null) => {
      await api.device(deviceId).stopSignIn(harnessId)
    },
    [api],
  )

  // ---- plugins ----

  const refreshPlugins = useCallback(async () => {
    try {
      setPlugins((await api.plugins()) ?? [])
    } catch {
      /* the poll heals */
    }
  }, [api])

  const installPlugin = useCallback(
    async (machineId, entry) => {
      const plugin = await api.installPlugin(machineId, entry)
      await refreshPlugins()
      return plugin
    },
    [api, refreshPlugins],
  )

  const removePlugin = useCallback(
    async (pluginId) => {
      await api.removePlugin(pluginId)
      await refreshPlugins()
    },
    [api, refreshPlugins],
  )

  const setPluginEnabled = useCallback(
    async (pluginId, enabled) => {
      await api.setPluginEnabled(pluginId, enabled)
      await refreshPlugins()
    },
    [api, refreshPlugins],
  )

  const acknowledgePluginKey = useCallback(
    async (pluginId) => {
      await api.acknowledgePluginKey(pluginId)
      await refreshPlugins()
    },
    [api, refreshPlugins],
  )

  // ---- automations ----

  const refreshAutomations = useCallback(async () => {
    try {
      setAutomations((await api.automations()) ?? [])
    } catch {
      /* the poll heals */
    }
  }, [api])

  const saveAutomation = useCallback(
    async (rule) => {
      const saved = await api.saveRule(rule)
      await refreshAutomations()
      return saved
    },
    [api, refreshAutomations],
  )

  const removeAutomation = useCallback(
    async (ruleId) => {
      await api.removeRule(ruleId)
      await refreshAutomations()
    },
    [api, refreshAutomations],
  )

  const enableAutomation = useCallback(
    async (ruleId, enabled) => {
      await api.setRuleEnabled(ruleId, enabled)
      await refreshAutomations()
    },
    [api, refreshAutomations],
  )

  // Run now - resolves with the chat the rule opened (the jump target).
  const runAutomation = useCallback(
    async (ruleId) => {
      const session = await api.runRule(ruleId)
      await refreshAutomations()
      return session
    },
    [api, refreshAutomations],
  )

  const resumeChat = useCallback(
    async (chatId) => {
      await api.resume(chatId)
    },
    [api],
  )

  const value = useMemo(
    () => ({
      api,
      connection,
      capabilities,
      sessions,
      machines,
      plugins,
      marketplace,
      automations,
      chats,
      openChat,
      openChatId,
      connect,
      disconnect,
      ensureChat,
      createChat,
      modelsFor,
      cloud,
      devices,
      thisDevice: cloud?.device?.id ?? null,
      apiFor,
      capabilitiesFor,
      refreshCloud,
      signInCloud,
      cancelCloudSignIn,
      signOutCloud,
      setDeviceKind,
      removeDevice,
      createEnrollment,
      deleteChat,
      syncChat,
      sendPrompt,
      updateChatSettings,
      answerDecision,
      stopChat,
      interruptChat,
      resumeChat,
      createMachine,
      removeMachine,
      installHarness,
      signIn,
      feedSignIn,
      stopSignIn,
      installPlugin,
      removePlugin,
      setPluginEnabled,
      acknowledgePluginKey,
      saveAutomation,
      removeAutomation,
      enableAutomation,
      runAutomation,
    }),
    [
      api,
      connection,
      capabilities,
      sessions,
      machines,
      plugins,
      marketplace,
      automations,
      chats,
      openChat,
      openChatId,
      connect,
      disconnect,
      ensureChat,
      createChat,
      modelsFor,
      cloud,
      devices,
      apiFor,
      capabilitiesFor,
      refreshCloud,
      signInCloud,
      cancelCloudSignIn,
      signOutCloud,
      setDeviceKind,
      removeDevice,
      createEnrollment,
      deleteChat,
      syncChat,
      sendPrompt,
      updateChatSettings,
      answerDecision,
      stopChat,
      interruptChat,
      resumeChat,
      createMachine,
      removeMachine,
      installHarness,
      signIn,
      feedSignIn,
      stopSignIn,
      installPlugin,
      removePlugin,
      setPluginEnabled,
      acknowledgePluginKey,
      saveAutomation,
      removeAutomation,
      enableAutomation,
      runAutomation,
    ],
  )
  return <ConsoleContext.Provider value={value}>{children}</ConsoleContext.Provider>
}

export function useConsole() {
  const context = useContext(ConsoleContext)
  if (!context) throw new Error('useConsole outside its provider')
  return context
}

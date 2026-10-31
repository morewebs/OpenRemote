// The console's connection and chat state: one daemon connection, a
// polled session list for the sidebar, and one live SSE subscription for
// the open chat (resuming from its cursor across reconnects).

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react'
import { DaemonApi, clearConnection, loadConnection, nextRetryDelay, sameDaemon, saveConnection, sessionGone } from '../api/daemon.js'
import { chatFromSession, foldEvents, mergeSessionIntoChat, vanishedChatIds } from './reducer.js'
import { createBeatLoop } from './beat.js'
import { samePayload } from './same.js'
import { errorCopy } from '../error-copy.js'
import { isMobile, openExternal } from '../platform.js'
import { loadArchived, saveArchived, withArchived } from '../archive.js'

const ConsoleContext = createContext(null)

const hasTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// How many sign-in poll beats in a row (800 ms each, about 30 s) may
// fail to read the relay's view before the relay is declared dead: one
// dropped read heals, only a view nobody can read over and over is a
// failure.
const SIGNIN_POLL_MISSES = 40

// How long a `connecting` state is shown as the app before it becomes the
// waiting screen. The daemon usually answers well inside this: a warm start
// has its address in the first `daemon_info` answer (a few milliseconds), and
// one that boots in half a second is connected before the timer fires. A
// longer grace would cover a slow cold boot (the registry probe spawns every
// harness before READY), but the waiting screen is patient copy rather than a
// verdict, so it is cheaper to show it early than to leave the app wordless.
const BOOT_GRACE_MS = 2500

// The frame the events wait for. A stream can deliver a replay of hundreds of
// events in one tick (a chat reopened, a reconnect, a fast harness), and every
// event used to copy the chat and its whole timeline. Queueing the events and
// folding the frame's batch against one copy is the same fold in the same
// order, published once per frame. The fallback keeps this working where
// there is no frame clock (a test, a headless harness).
const nextFrame =
  typeof requestAnimationFrame === 'function' ? requestAnimationFrame : (fn) => setTimeout(fn, 16)
const cancelFrame =
  typeof cancelAnimationFrame === 'function' ? cancelAnimationFrame : (id) => clearTimeout(id)

export function ConsoleProvider({ children }) {
  const [api, setApi] = useState(() => {
    // In the desktop shell the daemon is the shell's child: its address comes
    // from `daemon_info`, and the address saved by the last run is a port
    // nothing listens on any more. Dialling it first would report a healthy
    // boot as a failure. Browser mode has only the saved address.
    const saved = hasTauri ? {} : loadConnection()
    return new DaemonApi(saved.url, saved.token)
  })
  // Which daemon an answer belongs to. A fetch that was in flight when the
  // connection changed comes back for the daemon that is no longer there,
  // and its answer must not land on the next one's lists. The ref is written
  // where the new api is made, not in an effect: an answer that lands in the
  // moment right after the switch has to see the new daemon already.
  const apiRef = useRef(api)
  const stillCurrent = useCallback((asked) => apiRef.current === asked, [])
  // The running beat, so the waiting screen's retry can ask now instead of
  // waiting out the interval.
  const beatLoopRef = useRef(null)
  const [connection, setConnection] = useState(() => {
    const saved = loadConnection()
    return { state: saved.url && saved.token ? 'connecting' : 'new', error: null }
  })
  // What the beat asks about at settle time: the state at the moment the
  // answer lands, never the value captured when the loop started.
  const connectionRef = useRef(connection.state)
  connectionRef.current = connection.state
  // Whether the boot chain has finished, so the beat can follow it rather than run
  // beside it: both read the same lists and neither guards on recency, so the boot's
  // slower answer could land after the beat's and put a deleted chat back.
  const [bootSettled, setBootSettled] = useState(false)
  // What the shell last said about the daemon, including why it is not
  // running. Null until it answers once.
  const [daemon, setDaemon] = useState(null)
  const [capabilities, setCapabilities] = useState(null)
  const [sessions, setSessions] = useState([])
  const [machines, setMachines] = useState([])
  const [mcpServers, setMcpServers] = useState([])
  // The Plugins page: the built-in harness plugins and what is installed.
  const [plugins, setPlugins] = useState(null)
  // Plugins > Marketplaces, and Discover's entries from their catalogs.
  const [marketplaces, setMarketplaces] = useState([])
  const [catalog, setCatalog] = useState(null)
  // The last plugins or marketplaces refresh that failed. Kept so the pane
  // can say so and offer a retry: a swallowed failure left the last good
  // list on screen, which reads as a healthy list that is simply stale.
  const [pluginsError, setPluginsError] = useState(null)
  // A marketplace refresh has its own failure: the two lists can be stale for
  // different reasons, and one success must not declare the other fine.
  const [marketplacesError, setMarketplacesError] = useState(null)
  const [automations, setAutomations] = useState([])
  // The registered projects - daemon-side, the one list every surface
  // reads (the sidebar's groups, the composer's picker, the pill).
  const [projects, setProjects] = useState([])
  // Whether each collection has answered at least once. An empty list means
  // "there are none" only after the daemon said so: before the first answer
  // the same [] would read as an empty state ("No rules yet") rather than a
  // list still on its way. Set by whichever read lands first, the boot's or
  // a beat's, and cleared when the console points at another daemon.
  const [machinesAnswered, setMachinesAnswered] = useState(false)
  const [sessionsAnswered, setSessionsAnswered] = useState(false)
  const [mcpServersAnswered, setMcpServersAnswered] = useState(false)
  const [automationsAnswered, setAutomationsAnswered] = useState(false)
  const [projectsAnswered, setProjectsAnswered] = useState(false)
  // The account's devices are the same kind of list, and the one this console
  // must not guess at: an unanswered list is empty, and an empty list has been
  // read as "your phone never joined" and as "that machine left your Cloud".
  // Unknown is not empty; only an answer may say there are none.
  const [devicesAnswered, setDevicesAnswered] = useState(false)
  // A daemon the console has not heard from yet: whatever list is left on
  // screen belongs to the previous one, so the flags go back to loading.
  const resetAnswered = useCallback(() => {
    setMachinesAnswered(false)
    setSessionsAnswered(false)
    setMcpServersAnswered(false)
    setAutomationsAnswered(false)
    setProjectsAnswered(false)
    setDevicesAnswered(false)
  }, [])
  const [chats, setChats] = useState({})
  const [modelsByHarness, setModelsByHarness] = useState({})
  // Cloud: this computer's account state, and the account's devices.
  const [cloud, setCloud] = useState(null)
  const [devices, setDevices] = useState([])
  const [capabilitiesByDevice, setCapabilitiesByDevice] = useState({})
  // The chats on screen: each holds a live stream while it is watched, so
  // several panes can show running agents at once.
  const [watched, setWatched] = useState([])
  // The chats whose stream is between attempts. The stream object owns the
  // retry - its delay, its timer, its close - and this is the mirror the screen
  // reads, so a pane header and a sidebar row can say "reconnecting" while a
  // transcript waits instead of the chat's status's "running" being the only
  // thing on screen. Set when a close schedules another attempt, cleared when
  // the stream answers, when it is closed, and when it stops trying (a deleted
  // session). A mirror, not a second truth: nothing here decides anything.
  const [reconnecting, setReconnecting] = useState({})
  const markReconnecting = useCallback((id, on) => {
    setReconnecting((current) => {
      if (on) return current[id] ? current : { ...current, [id]: true }
      if (!current[id]) return current
      const next = { ...current }
      delete next[id]
      return next
    })
  }, [])
  const [archived, setArchived] = useState(loadArchived)
  const streamsRef = useRef(new Map())
  const chatsRef = useRef({})
  // The events of the frame being gathered, one queue per chat, and the frame
  // that will fold them. See nextFrame above for why they wait.
  const pendingRef = useRef(new Map())
  const frameRef = useRef(null)
  // Ids a poll has carried at least once - what tells a chat gone from
  // this beat (deleted elsewhere) from one not yet in any beat (created
  // between polls, fresher than the list).
  const polledIdsRef = useRef(new Set())

  // Everything one connection's daemon answered, back to the empty state.
  // Pointing the console at another daemon must not leave that daemon's
  // machines, rules, plugins or devices on screen as if they were its own:
  // the boot fills what it can, and the beat heals whatever failed.
  const resetDaemonData = useCallback(() => {
    setCapabilities(null)
    setMachines([])
    setMcpServers([])
    setPlugins(null)
    setMarketplaces([])
    setCatalog(null)
    setAutomations([])
    setProjects([])
    setCloud(null)
    setDevices([])
    setCapabilitiesByDevice({})
    // The chat list belongs to the daemon as much as anything above it. Leaving
    // these three to the leave path alone meant that pointing the console at another
    // daemon kept the previous one's chats on screen until the first beat replaced
    // them, and a chat clicked in that window opened a stream against a daemon that
    // never had the session.
    setSessions([])
    setChats({})
    setWatched([])
    // A new daemon means a new boot: the beat must wait for it again rather than
    // race it with the previous chain's flag still set.
    setBootSettled(false)
    resetAnswered()
  }, [resetAnswered])

  const connect = useCallback((url, token) => {
    // A catalog belongs to the daemon that answered it: the next connection
    // may be another machine's daemon, with other harnesses and endpoints.
    commandCache.current.clear()
    setModelsByHarness({})
    setEndpointsByHarness({})
    resetDaemonData()
    saveConnection(url, token)
    const next = new DaemonApi(url, token)
    apiRef.current = next
    setApi(next)
    setConnection({ state: 'connecting', error: null })
  }, [resetDaemonData])

  const disconnect = useCallback(() => {
    // The same, on the way out: nothing this daemon answered may be shown
    // for the next one.
    commandCache.current.clear()
    setModelsByHarness({})
    setEndpointsByHarness({})
    resetDaemonData()
    clearConnection()
    const next = new DaemonApi(null, null)
    apiRef.current = next
    setApi(next)
    setConnection({ state: 'new', error: null })
    setSessions([])
    setChats({})
    setWatched([])
  }, [resetDaemonData])

  // One way in for everything the shell says about the daemon, whether it
  // arrives as an answer to daemon_info or on the shell's own event. A
  // daemon that is up is connected to; a reported failure puts the app on
  // the waiting screen, and never over a working connection (the shell
  // clears its reason the moment a daemon answers).
  const applyDaemonInfo = useCallback(
    (info) => {
      if (!info) return
      setDaemon(info)
      if (info.url && info.token) {
        // The same daemon, reported again (a spawn that reused its port, a
        // retry that found it already up): re-pointing at it would tear down
        // every live stream and replay every transcript for no change. The
        // boot and the beat are already dialing that address, so the report
        // is news about nothing.
        if (!sameDaemon(apiRef.current, info.url, info.token)) connect(info.url, info.token)
        return
      }
      if (info.error) {
        setConnection((c) => (c.state === 'connected' ? c : { state: 'waiting-daemon', error: null }))
      }
    },
    [connect],
  )

  // The waiting screen's button: start the sidecar again now, instead of
  // waiting out the shell's backoff, which grows to half a minute.
  const retryDaemon = useCallback(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('daemon_restart')
      setDaemon((d) => (d ? { ...d, error: null } : d))
      // A retry takes the pending beat's place: the answer is worth having
      // now, not up to one interval from now.
      beatLoopRef.current?.kick()
    } catch {
      /* the screen keeps saying what it said */
    }
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
      let info = null
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        info = await invoke('daemon_info')
      } catch {
        /* the shell will say when it can; keep asking all the same */
      }
      if (cancelled) return
      if (info?.url && info?.token) {
        applyDaemonInfo(info)
        return
      }
      // Not up yet: take what the shell said (it may be the reason it
      // could not start) and keep asking for a while. A freshly built exe
      // can take a slow first run, so nothing is called a failure on the
      // strength of the clock alone.
      applyDaemonInfo(info)
      if (!info?.error && attempt < 150) {
        attempt += 1
        setTimeout(ask, 600)
        return
      }
      // Either the shell already knows it cannot start, or 90 seconds went
      // by with no answer at all: show the waiting screen. The poll below
      // carries on from here and leaves it the moment a daemon answers.
      setConnection((c) => (c.state === 'connected' ? c : { state: 'waiting-daemon', error: null }))
    }
    ask()
    // A daemon that is booting has no screen of its own: `connecting` renders
    // the app with nothing said, and the shell's own verdict is ninety
    // seconds away. The waiting screen is patient (no verdict, no blame) and
    // leaves by itself when the daemon answers, so it is the honest thing to
    // show while the shell has no address. It is only ever entered from
    // `connecting`: a first run starts in `new` and shows the wizard.
    //
    // `grace` marks this patience, because it is not a failure: nothing has
    // said anything went wrong, and the screen must not offer the button that
    // restarts the sidecar (that would throw a healthy boot away). Only the
    // shell's own reason clears the room for it (DaemonStatus).
    const slow = setTimeout(
      () =>
        setConnection((c) =>
          c.state === 'connecting' ? { state: 'waiting-daemon', error: null, grace: true } : c,
        ),
      BOOT_GRACE_MS,
    )
    return () => {
      cancelled = true
      clearTimeout(slow)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // The desktop shell owns the sidecar: while there is no connection, keep
  // asking the shell for the daemon's address - a sidecar that came back
  // on a new port is followed without a reload, and one that appears late
  // is picked up instead of needing a restart. This runs for the waiting
  // screen too, not only for an error: a sidecar still being written when
  // the app first started is the common way to end up there. The browser
  // has only the address it was given; its error copy says so.
  useEffect(() => {
    if (!hasTauri) return
    if (connection.state !== 'error' && connection.state !== 'waiting-daemon') return
    let cancelled = false
    const ask = async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const info = await invoke('daemon_info')
        if (cancelled) return
        applyDaemonInfo(info)
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

  // And the shell says so the moment the answer changes, on the same event
  // bus the browser reports its tabs on. A message sent before this
  // listener exists is not lost: the polls above ask anyway.
  useEffect(() => {
    if (!hasTauri) return
    let cancelled = false
    let unlisten = null
    ;(async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event')
        unlisten = await listen('daemon:status', ({ payload }) => applyDaemonInfo(payload))
        if (cancelled) {
          unlisten()
          unlisten = null
        }
      } catch {
        /* the polls cover it */
      }
    })()
    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Verify the connection, then load what the daemon has. Every read settles
  // on its own: one route that answers an error (a half-written MCP config, a
  // rules file the daemon cannot parse, a harness list mid-install) must not
  // cost the console its session list, its capabilities, or the connection
  // itself. The connection follows the health lists alone - the same four the
  // beat watches (BEAT_LISTS in beat.js) - and a beat refills whatever failed
  // here, so an unhappy route catches up without a restart.
  // A boot chain belongs to a connection and to whether that connection has
  // failed, not to the state it is showing. Keying this effect on
  // `connection.state` made the grace's own flip (connecting to the waiting
  // screen) cancel an in-flight chain and start a second one: a daemon that was
  // merely slow got asked twice, and could connect later than its first chain
  // would have. The flag below changes only when a boot itself fails, so the
  // grace and the other patient states leave a running chain alone.
  const bootFailed = connection.state === 'error'
  useEffect(() => {
    if (!api.ready || connection.state === 'connected') return undefined
    let cancelled = false
    ;(async () => {
      const [health, caps, list, machineList, mcpList, pluginList, marketList, catalogList, rules, projectList, cloudView, deviceList] =
        await Promise.allSettled([
          api.health(),
          api.capabilities(),
          api.sessions(),
          api.machines(),
          api.mcpServers(),
          api.plugins(),
          api.marketplaces(),
          api.marketplaceCatalog(),
          api.automations(),
          api.projects(),
          api.cloud(),
          api.cloudDevices(),
        ])
      if (cancelled || !stillCurrent(api)) return
      const healthLists = [list, machineList, mcpList, rules]
      if (!healthLists.some((result) => result.status === 'fulfilled')) {
        // Nothing the console reads as health arrived: the daemon is
        // unreachable or refusing. The words are the ones the serial chain
        // used, so the waiting screen says the same thing it did before.
        const first = [...healthLists, health].find((result) => result.status === 'rejected')
        const err = first?.reason
        const unreachable = err instanceof TypeError
        setConnection({
          state: 'error',
          error: unreachable
            ? "The daemon didn't answer - check that it's running, or reconnect from Settings."
            : err?.message ?? String(err),
        })
        // A boot that failed is exactly when the beat is needed: it is what turns a
        // waiting screen back into a connected one without a restart.
        setBootSettled(true)
        return
      }
      if (caps.status === 'fulfilled') setCapabilities(caps.value ?? null)
      if (cloudView.status === 'fulfilled') setCloud(cloudView.value)
      if (deviceList.status === 'fulfilled') {
        setDevices(deviceList.value?.devices ?? [])
        setDevicesAnswered(true)
      }
      if (list.status === 'fulfilled') {
        setSessions(list.value ?? [])
        setSessionsAnswered(true)
      }
      if (machineList.status === 'fulfilled') {
        setMachines(machineList.value ?? [])
        setMachinesAnswered(true)
      }
      if (mcpList.status === 'fulfilled') {
        setMcpServers(mcpList.value ?? [])
        setMcpServersAnswered(true)
      }
      if (rules.status === 'fulfilled') {
        setAutomations(rules.value ?? [])
        setAutomationsAnswered(true)
      }
      if (projectList.status === 'fulfilled') {
        setProjects(projectList.value ?? [])
        setProjectsAnswered(true)
      }
      if (pluginList.status === 'fulfilled') setPlugins(pluginList.value)
      if (marketList.status === 'fulfilled') setMarketplaces(marketList.value ?? [])
      if (catalogList.status === 'fulfilled') setCatalog(catalogList.value)
      setConnection({ state: 'connected', error: null })
      // The first paint is done; the beat owns every list from here.
      setBootSettled(true)
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, bootFailed])

  // The beat: the sidebar's truth, machines, MCP servers and rules ride it,
  // and capabilities ride it too so a harness installed, removed or signed in
  // after the connection appears without a restart. The cadence and the death
  // rule live in beat.js, tested there with mocked timers; this effect owns
  // the calls and the state writes. A beat never starts before the previous
  // one settled, so a slow daemon is not asked again while its answer is
  // still out, and three silent beats still trip the banner.
  //
  // It waits for the boot chain to settle first. Both read the same lists, and
  // `stillCurrent` is identity of the daemon rather than recency of the answer, so
  // an answer the boot started first could land last and put back a chat the beat
  // had just seen deleted. The boot's own reads are the first paint; the beat owns
  // every list after it.
  useEffect(() => {
    if (!api.ready || !bootSettled) return undefined
    const beat = createBeatLoop({
      currentConnection: () => connectionRef.current,
      run: async () => {
        const [sessionList, machineList, mcpList, ruleList, projectList, cloudView, deviceList, caps] =
          await Promise.allSettled([
            api.sessions(),
            api.machines(),
            api.mcpServers(),
            api.automations(),
            api.projects(),
            api.cloud(),
            api.cloudDevices(),
            api.capabilities(),
          ])
        // A beat from the daemon this console has left decides nothing.
        if (!stillCurrent(api)) return null
        if (cloudView.status === 'fulfilled') setCloud(cloudView.value)
        if (deviceList.status === 'fulfilled') {
          setDevices(deviceList.value?.devices ?? [])
          setDevicesAnswered(true)
        }
        if (sessionList.status === 'fulfilled') {
        setSessions(sessionList.value ?? [])
        setSessionsAnswered(true)
      }
        if (machineList.status === 'fulfilled') {
          setMachines(machineList.value ?? [])
          setMachinesAnswered(true)
        }
        if (mcpList.status === 'fulfilled') {
          setMcpServers(mcpList.value ?? [])
          setMcpServersAnswered(true)
        }
        if (ruleList.status === 'fulfilled') {
          setAutomations(ruleList.value ?? [])
          setAutomationsAnswered(true)
        }
        if (projectList.status === 'fulfilled') {
          setProjects(projectList.value ?? [])
          setProjectsAnswered(true)
        }
        // Harness status words move with installs and sign-ins. A fresh
        // payload replaces the old one; an identical one keeps its identity,
        // because a new array every 2.5 s re-renders every harness row.
        if (caps.status === 'fulfilled') {
          setCapabilities((current) => (samePayload(current, caps.value) ? current : caps.value ?? null))
        }
        return {
          sessions: sessionList,
          machines: machineList,
          mcpServers: mcpList,
          automations: ruleList,
          projects: projectList,
          cloud: cloudView,
          cloudDevices: deviceList,
          capabilities: caps,
        }
      },
      onBeat: ({ connection: next }) => {
        if (next === 'connected') setConnection({ state: 'connected', error: null })
        else if (next === 'error') {
          setConnection({
            state: 'error',
            error: 'The daemon stopped answering - it may have exited. Restart the app, or reconnect from Settings.',
          })
        }
      },
    })
    beatLoopRef.current = beat
    beat.start()
    return () => {
      beat.stop()
      if (beatLoopRef.current === beat) beatLoopRef.current = null
    }
  }, [api, stillCurrent, bootSettled])

  // Keep chat records in step with the polled session list: merge
  // identities (status/workspace) without disturbing folded transcripts.
  // The merge decision lives in reducer.js (mergeSessionIntoChat) so its
  // preserved-field contract is tested: event-folded facts the session
  // payload doesn't carry (the timeline, the context numbers, the
  // running cost and thinking-token totals, the decisions list) survive
  // every beat - the stream owns them. A record whose session left the
  // list goes with it (vanishedChatIds): its pane falls back to "no
  // longer in this workspace", its watch drops, and its stream stops
  // asking for a session the daemon answers 404 for.
  useEffect(() => {
    const polled = polledIdsRef.current
    for (const session of sessions) polled.add(session.id)
    setChats((current) => {
      const next = { ...current }
      for (const session of sessions) {
        const existing = next[session.id]
        if (!existing) continue
        next[session.id] = mergeSessionIntoChat(existing, session)
      }
      for (const id of vanishedChatIds(next, sessions, polled)) delete next[id]
      return next
    })
  }, [sessions])

  chatsRef.current = chats

  // Watch exactly these chats. Order is irrelevant; the same set is a no-op.
  const watchChats = useCallback((ids) => {
    setWatched((current) => {
      const next = [...new Set(ids)].sort()
      return current.length === next.length && current.every((id, i) => id === next[i])
        ? current
        : next
    })
  }, [])

  // One published state per frame: the queued events are folded, in the order
  // they arrived, against one copy of each chat and its timeline. The fold is
  // `foldEvents` - the same `foldEvent` calls, in the same sequence - so the
  // transcript is the one per-event folding produced (reducer.test.js folds a
  // recorded stream both ways and compares).
  const flushEvents = useCallback(() => {
    frameRef.current = null
    const queued = pendingRef.current
    if (queued.size === 0) return
    pendingRef.current = new Map()
    setChats((current) => {
      let next = null
      for (const [id, events] of queued) {
        const chat = (next ?? current)[id]
        if (!chat) continue
        const folded = foldEvents({ ...chat, timeline: [...chat.timeline] }, events)
        next = next === null ? { ...current, [id]: folded } : { ...next, [id]: folded }
      }
      return next ?? current
    })
  }, [])

  const queueEvent = useCallback(
    (id, event) => {
      const queued = pendingRef.current
      const list = queued.get(id)
      if (list) list.push(event)
      else queued.set(id, [event])
      if (frameRef.current === null) frameRef.current = nextFrame(flushEvents)
    },
    [flushEvents],
  )

  // One stream per watched chat: load its decisions, replay what it hasn't
  // folded yet, then follow it live, reconnecting from its cursor. The
  // cursor is the fold's (`foldedSeq`), never a separate transport count: an
  // event that arrived while the chat had no record was not folded, and a
  // transport cursor would move past it and into a permanent gap.
  const openStream = useCallback(
    (id) => {
      const stream = { cancelled: false, stop: null, retry: null, retryDelay: 0 }
      const apply = (event) => {
        // The wire is talking again: the next close starts the ladder over,
        // and the screen stops saying it is waiting for one.
        stream.retryDelay = 0
        markReconnecting(id, false)
        // The fold waits for the frame: a replay hands over hundreds of events
        // in one tick, and the copy belongs to the frame, not each event.
        queueEvent(id, event)
      }
      const follow = (after) => {
        stream.stop = api.events(id, {
          after,
          onEvent: apply,
          onClose: (_cursor, err) => {
            if (stream.cancelled) return
            // A deleted session is terminal (the daemon answered "no
            // such session"): without this check the pane kept dialing
            // the dead id every 800ms forever. The pane lets go of the
            // chat on its own once the polled session list drops it.
            if (sessionGone(err)) {
              // No further attempt comes: the row must not keep saying it does.
              markReconnecting(id, false)
              return
            }
            // A refusal the daemon will keep refusing is not a drop to retry. A rotated
            // token or a forbid answered every redial in five seconds while the row said
            // "reconnecting", a word that promises a reconnection that cannot come. 429
            // is the one 4xx that means "later".
            const refused =
              typeof err?.status === 'number' &&
              err.status >= 400 &&
              err.status < 500 &&
              err.status !== 429
            if (refused) {
              markReconnecting(id, false)
              // Written by the console, so it carries its own id and no seq: a
              // synthesized seq would move the fold cursor and a synthesized `at` would
              // outrank the poll beat.
              queueEvent(id, {
                type: 'daemon.error',
                id: `refused-${Date.now()}`,
                seq: null,
                message: errorCopy(err),
              })
              return
            }
            // The daemon replays everything past the client's fold cursor.
            // The wait doubles on every close in a row, so a daemon that
            // keeps dying is not dialed twice a second forever, and a
            // stream that talked resets it.
            stream.retryDelay = nextRetryDelay(stream.retryDelay)
            // The stream owns the wait; the screen is told it is happening.
            markReconnecting(id, true)
            stream.retry = setTimeout(() => {
              if (!stream.cancelled) follow(chatsRef.current[id]?.foldedSeq ?? null)
            }, stream.retryDelay)
          },
        })
      }
      ;(async () => {
        try {
          const list = await api.decisions(id)
          if (stream.cancelled) return
          setChats((current) => {
            const chat = current[id]
            if (!chat) return current
            return { ...current, [id]: { ...chat, decisionsList: list ?? [] } }
          })
        } catch {
          /* decisions load is best-effort; the stream carries them anyway */
        }
      })()
      // A chat shown again after a while already holds its timeline;
      // `after: null` would fold the whole history in a second time.
      follow(chatsRef.current[id]?.foldedSeq ?? null)
      stream.close = () => {
        stream.cancelled = true
        clearTimeout(stream.retry)
        stream.stop?.()
        // Events the frame had not folded yet are dropped with the stream: the
        // chat is not watched any more, and the cursor still sits at the last
        // folded seq, so a later open asks for them again and folds them once.
        pendingRef.current.delete(id)
        // A closed stream is not backing off, whatever it was doing: the row
        // and the header stop saying so with it.
        markReconnecting(id, false)
      }
      return stream
    },
    [api, queueEvent, markReconnecting],
  )

  // A new connection (or none) drops every stream; the next effect reopens
  // the watched set on it.
  useEffect(() => {
    const streams = streamsRef.current
    return () => {
      for (const stream of streams.values()) stream.close()
      streams.clear()
      pendingRef.current.clear()
      if (frameRef.current !== null) {
        cancelFrame(frameRef.current)
        frameRef.current = null
      }
    }
  }, [connection.state, openStream])

  useEffect(() => {
    if (connection.state !== 'connected') return
    const streams = streamsRef.current
    const want = new Set(watched)
    for (const [id, stream] of streams) {
      if (!want.has(id)) {
        stream.close()
        streams.delete(id)
      }
    }
    for (const id of want) if (!streams.has(id)) streams.set(id, openStream(id))
  }, [watched, connection.state, openStream])

  const ensureChat = useCallback(
    (session) => {
      // Merge into the map - the record never replaces it. Idempotent, so
      // the shell can ensure the chat for a hash-reached view too.
      setChats((current) =>
        current[session.id]
          ? current
          : { ...current, [session.id]: chatFromSession(session) },
      )
    },
    [],
  )

  // A new chat. In Cloud it syncs to every device, and with `deviceId` it
  // runs on that machine (created there, copied here); the prompt that
  // follows reaches it through this daemon either way.
  const createChat = useCallback(
    async (
      text, workspace, harness, model, fast, effort, permissionMode,
      { deviceId = null, synced = false, mentionedChats = [], endpointId = null } = {},
    ) => {
      const session = await api.createSession({
        harness, workspace, model, effort, fast, permissionMode, endpointId, deviceId, synced,
      })
      ensureChat(session)
      // The prompt can take a moment. The caller opens the chat from the id
      // as soon as the session exists, so a slow or failed send does not
      // leave the person on New chat. A send that fails is written into that
      // chat rather than swallowed here: the person is already looking at it,
      // and an agent that never answered without a word about why is the one
      // failure this console must not ship.
      if (text.trim()) {
        api.prompt(session.id, text.trim(), mentionedChats).catch((err) => {
          queueEvent(session.id, {
            type: 'daemon.error',
            id: `send-failed-${Date.now()}`,
            seq: null,
            message: errorCopy(err),
          })
        })
      }
      return session.id
    },
    [api, ensureChat, queueEvent],
  )

  // What a composer's / and @ menus offer, kept per chat (or per harness
  // and folder) for this console's life; a plugin installed, removed or
  // turned on or off clears it, since the lists carry the plugins.
  const commandCache = useRef(new Map())
  useEffect(() => {
    commandCache.current.clear()
  }, [plugins])
  const commandsFor = useCallback(
    async ({ harness, workspace = null, session = null, deviceId = null }) => {
      const key = `${deviceId ?? ''}|${session ?? `${harness}|${workspace ?? ''}`}`
      const cached = commandCache.current.get(key)
      if (cached) return cached
      const asked = api
        .device(deviceId)
        .commands({ harness, workspace, session })
        .catch((err) => {
          commandCache.current.delete(key)
          throw err
        })
      commandCache.current.set(key, asked)
      return asked
    },
    [api],
  )

  const filesFor = useCallback(
    async ({ workspace = null, session = null, deviceId = null, q = '', signal = null }) =>
      api.device(deviceId).files({ workspace, session, q }, { signal }),
    [api],
  )

  const renameChat = useCallback(async (chatId, title) => api.rename(chatId, title), [api])

  // The API of another of the user's devices, through this daemon.
  const apiFor = useCallback((deviceId) => api.device(deviceId), [api])

  // The model catalog for a harness, cached per connection and device.
  // Empty means the harness advertises nothing - the picker stays hidden.
  const modelsFor = useCallback(
    async (harnessId, deviceId = null) => {
      const key = `${deviceId ?? ''}:${harnessId}`
      // An empty answer is not cached: a machine that was still starting
      // up gets asked again.
      if (!api.ready || modelsByHarness[key]?.length) return modelsByHarness[key] ?? []
      try {
        const list = (await api.device(deviceId).models(harnessId)) ?? []
        // An answer from a daemon this console has left describes another
        // machine's catalog: the picker must not offer it, and the cache must
        // not keep it (it is only ever filled once per key).
        if (!stillCurrent(api)) return list
        if (list.length) setModelsByHarness((current) => ({ ...current, [key]: list }))
        return list
      } catch {
        return []
      }
    },
    [api, modelsByHarness, stillCurrent],
  )

  // The custom model endpoints on a machine, metadata only (the keys
  // never leave the daemon). Cached per device and harness; saving or
  // removing refreshes the row.
  const [endpointsByHarness, setEndpointsByHarness] = useState({})
  const endpointsFor = useCallback(
    async (harnessId, deviceId = null) => {
      const key = `${deviceId ?? ''}:${harnessId}`
      if (!api.ready) return endpointsByHarness[key] ?? []
      try {
        const list = (await api.device(deviceId).modelEndpoints({ harness: harnessId })) ?? []
        if (!stillCurrent(api)) return list
        setEndpointsByHarness((current) => ({ ...current, [key]: list }))
        return list
      } catch {
        return endpointsByHarness[key] ?? []
      }
    },
    [api, endpointsByHarness, stillCurrent],
  )

  const refreshEndpoints = useCallback(
    async (harnessId, deviceId = null) => {
      const key = `${deviceId ?? ''}:${harnessId}`
      try {
        const list = (await api.device(deviceId).modelEndpoints({ harness: harnessId })) ?? []
        if (!stillCurrent(api)) return list
        setEndpointsByHarness((current) => ({ ...current, [key]: list }))
        return list
      } catch {
        return endpointsByHarness[key] ?? []
      }
    },
    [api, endpointsByHarness, stillCurrent],
  )

  // The models one custom endpoint lists today, fetched live through
  // its own key. Failures are not cached - a gateway that is still
  // starting answers again next time.
  const endpointModelsFor = useCallback(
    async (endpointId, deviceId = null) => {
      if (!api.ready || !endpointId) return []
      try {
        return (await api.device(deviceId).endpointModels(endpointId)) ?? []
      } catch {
        return []
      }
    },
    [api],
  )

  const setChatEndpoint = useCallback(
    async (chatId, endpointId = null, deviceId = null) => {
      const session = await api.device(deviceId).setSessionEndpoint(chatId, endpointId)
      ensureChat(session)
      return session
    },
    [api, ensureChat],
  )

  // What another device can run, fetched when a new chat aims at it.
  const capabilitiesFor = useCallback(
    async (deviceId) => {
      if (!deviceId) return capabilities
      try {
        const caps = await api.device(deviceId).capabilities()
        if (!stillCurrent(api)) return caps
        setCapabilitiesByDevice((current) => ({ ...current, [deviceId]: caps }))
        return caps
      } catch {
        return capabilitiesByDevice[deviceId] ?? null
      }
    },
    [api, capabilities, capabilitiesByDevice, stillCurrent],
  )

  const refreshCloud = useCallback(async () => {
    const [view, list] = await Promise.allSettled([api.cloud(), api.cloudDevices()])
    if (!stillCurrent(api)) return
    if (view.status === 'fulfilled') setCloud(view.value)
    if (list.status === 'fulfilled') {
      setDevices(list.value?.devices ?? [])
      setDevicesAnswered(true)
    }
  }, [api, stillCurrent])

  // Sign-in happens in the system browser, which the daemon opens; the
  // poll picks up the result when the browser comes back. On a phone the
  // shell opens it instead, in a browser tab that returns to the app.
  const signInCloud = useCallback(async () => {
    const started = await api.cloudSignIn({ open: !isMobile })
    if (isMobile && started?.authorize_url) {
      // What the shell reports, not that the call was made: the browser can
      // still refuse the page, and then the caller has a link to show.
      started.opened = await openExternal(started.authorize_url, { signIn: true })
    }
    await refreshCloud()
    return started
  }, [api, refreshCloud])

  const cancelCloudSignIn = useCallback(async () => {
    setCloud(await api.cloudSignInCancel())
  }, [api])

  const signOutCloud = useCallback(async () => {
    setCloud(await api.cloudSignOut())
    setDevices([])
    // The devices list is not "empty" after a sign-out: it is unknown again,
    // which is the same thing it was before the first answer arrived.
    setDevicesAnswered(false)
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
    if (!hasTauri || isMobile || thisKind == null) return
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

  // Archiving is console-side: the chat leaves the sidebar, its transcript
  // and its other devices are untouched. The answer is whether the set reached storage,
  // so a surface can say a write was refused instead of leaving the row unchanged.
  const setChatArchived = useCallback((id, value) => {
    let saved = true
    setArchived((current) => {
      const next = withArchived(current, id, value)
      saved = saveArchived(next)
      return next
    })
    return saved
  }, [])

  // Deleting a synced chat removes it from every device.
  const deleteChat = useCallback(
    async (id) => {
      await api.deleteSession(id)
      setChatArchived(id, false)
      setSessions((current) => current.filter((s) => s.id !== id))
      setChats((current) => {
        const next = { ...current }
        delete next[id]
        return next
      })
    },
    [api, setChatArchived],
  )

  const sendPrompt = useCallback(
    async (chatId, text, mentionedChats = []) => {
      await api.prompt(chatId, text.trim(), mentionedChats)
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

  // The chat's queue (the daemon keeps it; queue.changed folds it here):
  // a message sent while a turn is working waits and runs when the turn
  // ends. Steer puts one into the running turn now.
  const queueMessage = useCallback(
    async (chatId, text, mentionedChats = []) => api.queue(chatId, text.trim(), mentionedChats),
    [api],
  )
  const removeQueued = useCallback(async (chatId, itemId) => api.queueRemove(chatId, itemId), [api])
  const steerQueued = useCallback(async (chatId, itemId) => api.queueSteer(chatId, itemId), [api])
  const steerMessage = useCallback(
    async (chatId, text, mentionedChats = []) => api.steer(chatId, text.trim(), mentionedChats),
    [api],
  )
  const resumeQueue = useCallback(async (chatId) => api.queueResume(chatId), [api])

  // ---- machines ----

  const refreshMachines = useCallback(async () => {
    try {
      const list = (await api.machines()) ?? []
      // A list from the daemon this console has left is another machine's
      // machines; the beat that owns the new one fills it.
      if (!stillCurrent(api)) return
      setMachines(list)
      setMachinesAnswered(true)
    } catch {
      /* the poll heals */
    }
  }, [api, stillCurrent])

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
  // when the CLI exits (or the relay is cancelled, or its poll gives up -
  // see below), and the capabilities are re-read so the row carries the
  // harness's own fresh status words.
  // A harness's own sign-in, relayed - on this computer, or on one of the
  // account's machines (`deviceId`), where its CLI runs.
  // Returns { promise, cancel }: `cancel` stops the poll (the modal's
  // cleanup) without touching the daemon's run - the human may still be
  // finishing in the browser. The poll also gives up on its own: a relay
  // whose view cannot be read SIGNIN_POLL_MISSES times in a row (about
  // 30 s - a daemon that restarted, a relay that never survived)
  // settles with the last error instead of being read every 800ms for
  // the app's lifetime.
  const signIn = useCallback(
    (harnessId, { onBeat, deviceId = null } = {}) => {
      const target = api.device(deviceId)
      let cancelled = false
      let stopPoll = null
      const cancelledError = () => {
        const err = new Error('Sign-in was cancelled')
        err.cancelled = true
        return err
      }
      const promise = (async () => {
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
          if (cancelled) throw cancelledError()
          start = await target.startSignIn(harnessId)
        }
        if (cancelled) throw cancelledError()
        const first = start?.result ?? start
        onBeat?.(first)
        if (first?.done) {
          await refreshCapabilities()
          return first
        }
        return await new Promise((resolve, reject) => {
          let misses = 0
          let lastError = null
          const poll = setInterval(async () => {
            let view = null
            try {
              view = await target.signInView(harnessId)
            } catch (err) {
              // A single dropped read heals; only a relay whose view
              // cannot be read at all, over and over, is a failure.
              misses += 1
              lastError = err
              if (misses >= SIGNIN_POLL_MISSES) {
                clearInterval(poll)
                reject(
                  lastError ??
                    new Error('The sign-in stopped answering - try again.'),
                )
              }
              return
            }
            misses = 0
            onBeat?.(view)
            if (view?.done) {
              clearInterval(poll)
              await refreshCapabilities()
              resolve(view)
            }
          }, 800)
          const stop = () => {
            clearInterval(poll)
            reject(cancelledError())
          }
          // A cancel that arrived before the poll existed (the modal
          // unmounted while startSignIn was still in flight).
          if (cancelled) stop()
          else stopPoll = stop
        })
      })()
      return {
        promise,
        cancel: () => {
          cancelled = true
          stopPoll?.()
        },
      }
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

  // The installed list and Discover's view of it (installed, update) move
  // together. A catalog is per daemon: an answer that arrives after the
  // connection moved on describes a daemon that is gone, so it is dropped.
  const refreshPlugins = useCallback(async () => {
    try {
      const [pluginList, catalogList] = await Promise.all([api.plugins(), api.marketplaceCatalog()])
      if (!stillCurrent(api)) return
      setPlugins(pluginList)
      setCatalog(catalogList)
      setPluginsError(null)
    } catch (err) {
      // Kept rather than swallowed: the next action may never come, and the
      // pane has to be able to say that what it shows is not current. The
      // sentence is error-copy's, so a daemon refusal is quoted as the
      // daemon's words rather than adopted as ours.
      setPluginsError(errorCopy(err))
    }
  }, [api, stillCurrent])

  const refreshMarketplaces = useCallback(async () => {
    try {
      const [marketList, catalogList] = await Promise.all([api.marketplaces(), api.marketplaceCatalog()])
      if (!stillCurrent(api)) return
      setMarketplaces(marketList ?? [])
      setCatalog(catalogList)
      setMarketplacesError(null)
    } catch (err) {
      // Its own field: a plugin refresh that succeeds afterwards says nothing
      // about the marketplaces, and used to clear this failure anyway.
      setMarketplacesError(errorCopy(err))
    }
  }, [api, stillCurrent])

  const addMarketplace = useCallback(
    async (source) => {
      const reply = await api.addMarketplace(source)
      await refreshMarketplaces()
      return reply
    },
    [api, refreshMarketplaces],
  )

  // A failed fetch is not thrown: the record carries its error, and the
  // cached catalog keeps serving Discover.
  const refreshMarketplace = useCallback(
    async (name) => {
      const reply = await api.refreshMarketplace(name)
      await refreshMarketplaces()
      return reply
    },
    [api, refreshMarketplaces],
  )

  const setMarketplaceEnabled = useCallback(
    async (name, enabled) => {
      await api.setMarketplaceEnabled(name, enabled)
      await refreshMarketplaces()
    },
    [api, refreshMarketplaces],
  )

  const removeMarketplace = useCallback(
    async (name) => {
      await api.removeMarketplace(name)
      await refreshMarketplaces()
    },
    [api, refreshMarketplaces],
  )

  const stagePlugin = useCallback(async (source) => api.stagePlugin(source), [api])
  const cancelStagedPlugin = useCallback(async (stageId) => api.cancelStagedPlugin(stageId), [api])

  const installStagedPlugin = useCallback(
    async (stageId) => {
      const reply = await api.installStagedPlugin(stageId)
      await refreshPlugins()
      return reply
    },
    [api, refreshPlugins],
  )

  // A built-in harness turned off leaves every picker - the pickers read
  // capabilities, so those refresh too.
  const setPluginEnabled = useCallback(
    async (id, enabled) => {
      await api.setPluginEnabled(id, enabled)
      await Promise.all([refreshPlugins(), refreshCapabilities()])
    },
    [api, refreshPlugins, refreshCapabilities],
  )

  const removePlugin = useCallback(
    async (id) => {
      await api.removePlugin(id)
      await refreshPlugins()
    },
    [api, refreshPlugins],
  )

  // ---- MCP servers ----

  const refreshMcpServers = useCallback(async () => {
    try {
      const list = (await api.mcpServers()) ?? []
      if (!stillCurrent(api)) return
      setMcpServers(list)
      setMcpServersAnswered(true)
    } catch {
      /* the poll heals */
    }
  }, [api, stillCurrent])

  const addMcpServer = useCallback(
    async (machineId, entry) => {
      const added = await api.addMcpServer(machineId, entry)
      await refreshMcpServers()
      return added
    },
    [api, refreshMcpServers],
  )

  const removeMcpServer = useCallback(
    async (id) => {
      await api.removeMcpServer(id)
      await refreshMcpServers()
    },
    [api, refreshMcpServers],
  )

  const setMcpServerEnabled = useCallback(
    async (id, enabled) => {
      await api.setMcpServerEnabled(id, enabled)
      await refreshMcpServers()
    },
    [api, refreshMcpServers],
  )

  const acknowledgeMcpServerKey = useCallback(
    async (id) => {
      await api.acknowledgeMcpServerKey(id)
      await refreshMcpServers()
    },
    [api, refreshMcpServers],
  )

  // ---- automations ----

  const refreshAutomations = useCallback(async () => {
    try {
      const list = (await api.automations()) ?? []
      if (!stillCurrent(api)) return
      setAutomations(list)
      setAutomationsAnswered(true)
    } catch {
      /* the poll heals */
    }
  }, [api, stillCurrent])

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

  // ---- projects ----

  const refreshProjects = useCallback(async () => {
    try {
      const list = (await api.projects()) ?? []
      if (!stillCurrent(api)) return
      setProjects(list)
      setProjectsAnswered(true)
    } catch {
      /* the poll heals */
    }
  }, [api, stillCurrent])

  // Add a project - one folder this release, registered on the machine
  // that owns it (`apiFor(deviceId)` aims there in Cloud).
  const addProject = useCallback(
    async (folders, { deviceId = null } = {}) => {
      const project = await api.device(deviceId).createProject({ folders })
      await refreshProjects()
      return project
    },
    [api, refreshProjects],
  )

  // Unregister a project - its chats stay, they just stop being grouped.
  const removeProject = useCallback(
    async (projectId) => {
      await api.removeProject(projectId)
      await refreshProjects()
    },
    [api, refreshProjects],
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
      mcpServers,
      plugins,
      stagePlugin,
      installStagedPlugin,
      cancelStagedPlugin,
      setPluginEnabled,
      removePlugin,
      marketplaces,
      catalog,
      pluginsError,
      marketplacesError,
      refreshPlugins,
      addMarketplace,
      refreshMarketplace,
      setMarketplaceEnabled,
      removeMarketplace,
      automations,
      projects,
      machinesAnswered,
      sessionsAnswered,
      mcpServersAnswered,
      automationsAnswered,
      projectsAnswered,
      devicesAnswered,
      chats,
      reconnecting,
      watchChats,
      connect,
      disconnect,
      ensureChat,
      createChat,
      modelsFor,
      endpointsFor,
      refreshEndpoints,
      endpointModelsFor,
      setChatEndpoint,
      cloud,
      devices,
      thisDevice: cloud?.device?.id ?? null,
      apiFor,
      commandsFor,
      filesFor,
      renameChat,
      capabilitiesFor,
      refreshCloud,
      signInCloud,
      cancelCloudSignIn,
      signOutCloud,
      setDeviceKind,
      removeDevice,
      createEnrollment,
      deleteChat,
      archived,
      setChatArchived,
      syncChat,
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
      createMachine,
      removeMachine,
      installHarness,
      signIn,
      feedSignIn,
      stopSignIn,
      addMcpServer,
      removeMcpServer,
      setMcpServerEnabled,
      acknowledgeMcpServerKey,
      saveAutomation,
      removeAutomation,
      enableAutomation,
      runAutomation,
      addProject,
      removeProject,
      daemon,
      retryDaemon,
    }),
    [
      api,
      connection,
      capabilities,
      sessions,
      machines,
      mcpServers,
      plugins,
      stagePlugin,
      installStagedPlugin,
      cancelStagedPlugin,
      setPluginEnabled,
      removePlugin,
      marketplaces,
      catalog,
      pluginsError,
      marketplacesError,
      refreshPlugins,
      addMarketplace,
      refreshMarketplace,
      setMarketplaceEnabled,
      removeMarketplace,
      automations,
      projects,
      machinesAnswered,
      sessionsAnswered,
      mcpServersAnswered,
      automationsAnswered,
      projectsAnswered,
      devicesAnswered,
      chats,
      reconnecting,
      watchChats,
      connect,
      disconnect,
      ensureChat,
      createChat,
      modelsFor,
      endpointsFor,
      refreshEndpoints,
      endpointModelsFor,
      setChatEndpoint,
      cloud,
      devices,
      apiFor,
      commandsFor,
      filesFor,
      renameChat,
      capabilitiesFor,
      refreshCloud,
      signInCloud,
      cancelCloudSignIn,
      signOutCloud,
      setDeviceKind,
      removeDevice,
      createEnrollment,
      deleteChat,
      archived,
      setChatArchived,
      syncChat,
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
      createMachine,
      removeMachine,
      installHarness,
      signIn,
      feedSignIn,
      stopSignIn,
      addMcpServer,
      removeMcpServer,
      setMcpServerEnabled,
      acknowledgeMcpServerKey,
      saveAutomation,
      removeAutomation,
      enableAutomation,
      runAutomation,
      addProject,
      removeProject,
      daemon,
      retryDaemon,
    ],
  )
  return <ConsoleContext.Provider value={value}>{children}</ConsoleContext.Provider>
}

export function useConsole() {
  const context = useContext(ConsoleContext)
  if (!context) throw new Error('useConsole outside its provider')
  return context
}

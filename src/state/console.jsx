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
  const [chats, setChats] = useState({})
  const [modelsByHarness, setModelsByHarness] = useState({})
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
          // slow first run) — keep asking for a long while yet.
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

  // Verify the connection, then load capabilities + sessions.
  useEffect(() => {
    if (!api.ready || connection.state === 'connected') return
    let cancelled = false
    ;(async () => {
      try {
        await api.health()
        const caps = await api.capabilities()
        const list = await api.sessions()
        if (cancelled) return
        setCapabilities(caps)
        setSessions(list ?? [])
        setConnection({ state: 'connected', error: null })
      } catch (err) {
        if (cancelled) return
        setConnection({ state: 'error', error: err.message ?? String(err) })
      }
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, connection.state])

  // Poll the session list while connected (the sidebar's live truth).
  useEffect(() => {
    if (connection.state !== 'connected') return
    const poll = setInterval(async () => {
      try {
        const list = await api.sessions()
        setSessions(list ?? [])
      } catch {
        /* the next poll heals; the open chat's stream reports harder */
      }
    }, 2500)
    return () => clearInterval(poll)
  }, [api, connection.state])

  // Keep chat records in step with the polled session list: merge
  // identities (status/workspace) without disturbing folded transcripts.
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
          pendingDecisionId: existing.pendingDecisionId,
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
        return { ...current, [openChatId]: foldEvent({ ...chat, timeline: [...chat.timeline] }, event) }
      })
    }
    const stop = api.events(openChatId, {
      after: null,
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
      setChats((current) => current[session.id] ?? chatFromSession(session))
      setOpenChatId(session.id)
    },
    [],
  )

  const createChat = useCallback(
    async (text, workspace, harness, model, fast) => {
      const session = await api.createSession({ harness, workspace, model, fast })
      ensureChat(session)
      if (text.trim()) await api.prompt(session.id, text.trim())
      return session.id
    },
    [api, ensureChat],
  )

  // The model catalog for a harness, cached per connection. Empty means
  // the harness advertises nothing — the picker stays hidden.
  const modelsFor = useCallback(
    async (harnessId) => {
      if (!api.ready || modelsByHarness[harnessId]) return modelsByHarness[harnessId] ?? []
      try {
        const list = (await api.models(harnessId)) ?? []
        setModelsByHarness((current) => ({ ...current, [harnessId]: list }))
        return list
      } catch {
        return []
      }
    },
    [api, modelsByHarness],
  )

  const sendPrompt = useCallback(
    async (chatId, text) => {
      await api.prompt(chatId, text.trim())
    },
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
      chats,
      openChat,
      openChatId,
      connect,
      disconnect,
      ensureChat,
      createChat,
      modelsFor,
      sendPrompt,
      answerDecision,
      stopChat,
      resumeChat,
    }),
    [
      api,
      connection,
      capabilities,
      sessions,
      chats,
      openChat,
      openChatId,
      connect,
      disconnect,
      ensureChat,
      createChat,
      modelsFor,
      sendPrompt,
      answerDecision,
      stopChat,
      resumeChat,
    ],
  )
  return <ConsoleContext.Provider value={value}>{children}</ConsoleContext.Provider>
}

export function useConsole() {
  const context = useContext(ConsoleContext)
  if (!context) throw new Error('useConsole outside its provider')
  return context
}

// The web console's device view: one machine of the account, its synced chats,
// and one chat open at a time.
//
// There is no local daemon in a browser, so this view reads what the machine's
// own copy holds, over the link, through the same DaemonApi the desktop uses.
// Two routes carry the whole view: the synced-chat summary, and a range of one
// chat's log. Nothing here asks for a stream, because a peer cannot be given
// one: the log is read again on a short timer while the chat is open.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowLeft, CaretRight, PaperPlaneRight, ArrowClockwise } from '@phosphor-icons/react'
import { DaemonApi, newRequestId } from '../api/daemon.js'
import { chatFromSession, foldEvents } from '../state/reducer.js'
import { renderMarkdown } from '../markdown.js'
import { SplashMark } from '../Splash.jsx'
import { mayPoll, pollDelay, visibilityAction } from './poll.js'
import { PEER_ORIGIN } from './transport.js'
import {
  EMPTY_SUMMARY,
  advanceCursor,
  answerLabel,
  answerRefusal,
  answerRequest,
  canAnswerDecision,
  canSend,
  chatRefusal,
  decisionOptions,
  groupByDay,
  headOf,
  hydrateTitles,
  pageSize,
  promptRequest,
  relativeWhen,
  rowLine,
  rowTitle,
  shortId,
  summaryList,
} from './sync-view.js'
import './device-view.css'

// What this view is, once, so nobody reads the list as cloud-side chats.
const WHAT_THIS_IS =
  'These chats live on this computer and keep running there. This page reads the latest events every few seconds; it is not a live token-by-token stream.'

const when = (ms) => {
  if (!ms) return ''
  const date = new Date(ms)
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}

// One transcript item. The console's reducer produces these shapes; this draws
// the ones a synced log can carry, and stays silent about anything else rather
// than inventing a row.
function Item({ item, onAnswer, answers, answeringId, answerProblems }) {
  if (item.kind === 'tool') {
    return (
      <details className={`dv-tool${item.state === 'failed' ? ' is-failed' : ''}`}>
        <summary>
          <span className="dv-tool-name">{item.name ?? 'a tool'}</span>
          <span className="dv-tool-state">{item.state === 'failed' ? 'failed' : item.result ? 'done' : 'running'}</span>
        </summary>
        {item.input != null && (
          <pre className="dv-pre">{typeof item.input === 'string' ? item.input : JSON.stringify(item.input, null, 2)}</pre>
        )}
        {item.result != null && <pre className="dv-pre dv-pre--result">{item.result}</pre>}
      </details>
    )
  }
  if (item.kind === 'decision') {
    const answered = answers[item.id] ?? (item.pending ? null : answerLabel(item))
    const busy = answeringId === item.id
    const options = decisionOptions(item)
    return (
      <div className={`dv-decision${item.pending && !answered ? ' is-pending' : ''}`}>
        <p className="dv-decision-line">
          {item.pending && !answered
            ? `Needs an answer on the computer: ${item.toolName ?? 'a decision'}`
            : `Answered: ${answered ?? item.answeredChoice ?? item.state ?? 'settled'}`}
        </p>
        {item.pending && !answered && options.length > 0 && (
          <div className="dv-choices">
            {options.map((option) => (
              <button
                key={String(option.id)}
                type="button"
                className="dv-btn"
                disabled={busy}
                onClick={() => onAnswer(item, option)}
              >
                {option.label ?? String(option.id)}
              </button>
            ))}
            {busy && <span className="dv-answering">Sending</span>}
          </div>
        )}
        {answerProblems[item.id] && <p className="dv-problem">{answerProblems[item.id]}</p>}
      </div>
    )
  }
  if (item.kind === 'reasoning' || item.kind === 'reasoning-stream') {
    return (
      <details className="dv-reasoning">
        <summary>Reasoning{item.kind === 'reasoning-stream' ? ' (being written)' : ''}</summary>
        <pre className="dv-pre">{item.text}</pre>
      </details>
    )
  }
  const role = item.role === 'user' ? 'user' : item.role === 'note' ? 'note' : 'agent'
  if (role === 'note') return <p className="dv-note">{item.text}</p>
  // The console's own markdown pipeline, so a reply looks the same here as in
  // the desktop app.
  return (
    <div className={`dv-msg dv-msg--${role}${item.kind === 'stream' ? ' is-live' : ''}`}>
      <div className="dv-md" dangerouslySetInnerHTML={{ __html: renderMarkdown(item.text) }} />
    </div>
  )
}

export default function DeviceView({ machine, peer, onLeave }) {
  const client = useMemo(() => new DaemonApi(`${PEER_ORIGIN}/`, 'web-link', '', peer.fetchFor(machine.id)), [machine.id, peer])
  const [raw, setRaw] = useState(null)
  const [titles, setTitles] = useState({})
  const [listProblem, setListProblem] = useState(null)
  const [openId, setOpenId] = useState(null)
  const [chat, setChat] = useState(null)
  const [chatProblem, setChatProblem] = useState(null)
  const [closed, setClosed] = useState(false)
  const [loadingChat, setLoadingChat] = useState(false)
  const [draft, setDraft] = useState('')
  const [sending, setSending] = useState(false)
  const [sendProblem, setSendProblem] = useState(null)
  // A decision answered here: the option's label, by decision id. It stands
  // until the fold sees the answered event, so a slow poll cannot draw the row
  // as pending again and invite a second answer.
  const [answers, setAnswers] = useState({})
  const [answeringId, setAnsweringId] = useState(null)
  const [answerProblems, setAnswerProblems] = useState({})

  const sessions = useMemo(() => summaryList(raw, titles), [raw, titles])
  const groups = useMemo(() => groupByDay(sessions), [sessions])
  const openRow = sessions.find((row) => row.id === openId) ?? null

  // The polling state lives in refs: the loop must not restart on a render,
  // and two polls at once would fold the same page into the transcript twice.
  const openRef = useRef(null)
  const chatRef = useRef(null)
  const cursor = useRef(0)
  const timer = useRef(null)
  const inFlight = useRef(false)
  const failures = useRef(0)
  const stopped = useRef(false)
  const alive = useRef(true)
  const visible = useRef(typeof document === 'undefined' ? true : document.visibilityState !== 'hidden')
  // The loop calls through a ref so the timer always reaches the current
  // function, never the one from the render that scheduled it.
  const pollRef = useRef(null)

  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      stopped.current = true
      clearTimeout(timer.current)
    }
  }, [])

  const loadSummary = useCallback(async () => {
    setListProblem(null)
    try {
      const payload = await client.call('GET', '/sync/summary', null, { timeoutMs: 15000 })
      if (alive.current) setRaw(payload)
    } catch (err) {
      // The daemon's own sentence, when it has one: "did not answer" is for the
      // case where nothing came back at all, and saying it about a refusal
      // hides the reason.
      const said = typeof err?.message === 'string' ? err.message.trim() : ''
      if (alive.current) {
        setListProblem(
          said && !/did not answer in time/.test(said)
            ? `The computer refused this request: ${said}`
            : 'The computer did not answer. Its agent may be asleep or offline.',
        )
      }
    }
  }, [client])

  useEffect(() => {
    void loadSummary()
  }, [loadSummary])

  // The summary carries no title. Read the first twelve chats' own logs once
  // per summary, three at a time, and stop when the page leaves or a refresh
  // replaces the list. A failed read leaves that row "Untitled chat".
  const hydrateEpoch = useRef(0)
  useEffect(() => {
    if (!raw || sessions.length === 0) return
    const epoch = ++hydrateEpoch.current
    const cancelled = () => epoch !== hydrateEpoch.current
    void hydrateTitles(
      sessions,
      async (row) => {
        const page = await client.call(
          'GET',
          `/sync/sessions/${encodeURIComponent(row.id)}/events?from=0`,
          null,
          { timeoutMs: 20000 },
        )
        const record = chatFromSession({
          id: row.id,
          title: null,
          status: 'idle',
          updated_at: row.updatedAt,
          executor: row.executor,
          events: [],
        })
        foldEvents(record, page?.events ?? [])
        return record.title
      },
      {
        isCancelled: cancelled,
        onTitle: (id, title) => {
          if (!cancelled()) {
            setTitles((known) => (known[id] === title ? known : { ...known, [id]: title }))
          }
        },
      },
    )
    return () => {
      hydrateEpoch.current += 1
    }
    // The summary is the trigger; titles changing must not restart the reads.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [raw, client])

  const fold = useCallback((page) => {
    const record = chatRef.current
    if (!record) return
    foldEvents(record, page?.events ?? [])
    cursor.current = advanceCursor(cursor.current, page)
    if (record.title) {
      setTitles((known) => (known[record.id] === record.title ? known : { ...known, [record.id]: record.title }))
    }
    // A fresh object is what tells React the transcript grew; the fold itself
    // mutates the record, the same contract the desktop console uses.
    setChat({ ...record })
  }, [])

  const schedule = useCallback((wait) => {
    clearTimeout(timer.current)
    timer.current = setTimeout(() => {
      void pollRef.current?.()
    }, wait)
  }, [])

  const poll = useCallback(async () => {
    clearTimeout(timer.current)
    const id = openRef.current
    if (!mayPoll({ open: Boolean(id), stopped: stopped.current, visible: visible.current, inFlight: inFlight.current })) return
    inFlight.current = true
    try {
      const page = await client.call('GET', `/sync/sessions/${encodeURIComponent(id)}/events?from=${cursor.current}`, null, {
        timeoutMs: 20000,
      })
      if (!alive.current) return
      failures.current = 0
      // A recovered poll takes the failure line down: the chat is being read
      // again, and a stale "did not answer" would read as still broken.
      setChatProblem((current) => (current ? null : current))
      fold(page)
      const head = headOf(page)
      if (pageSize(page) === 0 && head != null) {
        // Nothing new since the cursor: the machine's own count is the only
        // thing to show, and it never moves the cursor.
        setChat((current) => (current ? { ...current, head } : current))
      }
      schedule(pollDelay(0))
    } catch (err) {
      if (!alive.current) return
      const refusal = chatRefusal(err)
      if (refusal.stop) {
        stopped.current = true
        setClosed(true)
        setChatProblem(refusal.sentence)
        return
      }
      failures.current += 1
      setChatProblem(refusal.sentence)
      schedule(pollDelay(failures.current))
    } finally {
      inFlight.current = false
    }
  }, [client, fold, schedule])

  useEffect(() => {
    pollRef.current = poll
  }, [poll])

  // A hidden tab does no work; a tab coming back catches up at once.
  useEffect(() => {
    const onChange = () => {
      const next = document.visibilityState !== 'hidden'
      visible.current = next
      if (visibilityAction(document.visibilityState) === 'pause') {
        clearTimeout(timer.current)
      } else if (openRef.current && !stopped.current) {
        void poll()
      }
    }
    document.addEventListener('visibilitychange', onChange)
    return () => document.removeEventListener('visibilitychange', onChange)
  }, [poll])

  const openChat = useCallback(
    async (row) => {
      clearTimeout(timer.current)
      stopped.current = false
      failures.current = 0
      cursor.current = 0
      openRef.current = row.id
      setOpenId(row.id)
      setClosed(false)
      setChat(null)
      setChatProblem(null)
      setSendProblem(null)
      setLoadingChat(true)
      const record = chatFromSession({
        id: row.id,
        title: row.title,
        status: 'idle',
        updated_at: row.updatedAt,
        executor: row.executor,
        events: [],
      })
      chatRef.current = record
      setChat({ ...record })
      try {
        const page = await client.call('GET', `/sync/sessions/${encodeURIComponent(row.id)}/events?from=0`, null, {
          timeoutMs: 20000,
        })
        if (!alive.current || openRef.current !== row.id) return
        failures.current = 0
        fold(page)
        schedule(0)
      } catch (err) {
        if (!alive.current) return
        const refusal = chatRefusal(err)
        if (refusal.stop) {
          stopped.current = true
          setClosed(true)
          setChatProblem(refusal.sentence)
        } else {
          failures.current += 1
          setChatProblem(refusal.sentence)
          schedule(pollDelay(failures.current))
        }
      } finally {
        if (alive.current) setLoadingChat(false)
      }
    },
    [client, fold, schedule],
  )

  const closeChat = useCallback(() => {
    stopped.current = true
    clearTimeout(timer.current)
    openRef.current = null
    chatRef.current = null
    setOpenId(null)
    setChat(null)
    setChatProblem(null)
    setClosed(false)
    void loadSummary()
  }, [loadSummary])

  const send = useCallback(async () => {
    const id = openRef.current
    if (!id || !canSend(draft) || sending) return
    setSending(true)
    setSendProblem(null)
    try {
      const request = promptRequest({ id, text: draft.trim(), requestId: newRequestId() })
      await client.call(request.method, request.path, request.body, { timeoutMs: 20000 })
      if (!alive.current) return
      setDraft('')
      // The person's own message should not wait for the next timer: catch up
      // now, once, and let the loop carry the reply.
      failures.current = 0
      await poll()
    } catch (err) {
      if (alive.current) setSendProblem(chatRefusal(err).sentence)
    } finally {
      if (alive.current) setSending(false)
    }
  }, [client, draft, poll, sending])

  // Answer a decision the way the desktop console does: one POST with the
  // daemon's receipt id and the harness's own choice id. The row says it is
  // sending while the request is out, cannot be sent twice, and an answered
  // fold takes over the moment the log carries the response.
  const answer = useCallback(
    async (item, option) => {
      const choice = option?.id ?? option
      const already = answers[item.id]
      if (
        !canAnswerDecision({
          pending: item.pending,
          answeredChoice: already ?? item.answeredChoice,
          answering: answeringId === item.id,
          choice,
        })
      ) {
        return
      }
      setAnsweringId(item.id)
      setAnswerProblems((known) => ({ ...known, [item.id]: null }))
      try {
        const request = answerRequest({ decisionId: item.id, choice, requestId: newRequestId() })
        await client.call(request.method, request.path, request.body, { timeoutMs: 20000 })
        if (!alive.current) return
        setAnswers((known) => ({ ...known, [item.id]: option?.label ?? String(choice) }))
        failures.current = 0
        await poll()
      } catch (err) {
        // The daemon's own sentence, when it gave one: "no such decision" is
        // more use than a generic failure, and the row stays answerable.
        if (alive.current) setAnswerProblems((known) => ({ ...known, [item.id]: answerRefusal(err) }))
      } finally {
        if (alive.current) setAnsweringId(null)
      }
    },
    [answers, answeringId, client, poll],
  )

  return (
    <div className="dv">
      <header className="dv-head">
        <div className="dv-head-inner">
          <button
            type="button"
            className="dv-icon"
            onClick={onLeave}
            title="Choose another machine"
            aria-label="Choose another machine"
          >
            <ArrowLeft size={16} weight="bold" />
          </button>
          <SplashMark className="dv-mark" />
          <span className="dv-brand">OpenRemote</span>
          <span className="dv-divider" aria-hidden="true" />
          <span className="dv-machine">{machine.name}</span>
          <span className="dv-spacer" />
          <span className={`dv-status${raw || machine.online ? ' is-online' : ''}`}>
            <span className="dv-dot" aria-hidden="true" />
            {raw ? 'Linked' : machine.online ? 'Online' : 'Offline'}
          </span>
          <button
            type="button"
            className="dv-icon"
            onClick={() => loadSummary()}
            title="Read the synced chats again"
            aria-label="Refresh"
          >
            <ArrowClockwise size={15} />
          </button>
        </div>
      </header>

      <main className="dv-main">
        {openId ? (
          <section className="dv-chat" aria-label="Chat">
            <div className="dv-chat-head">
              <button
                type="button"
                className="dv-icon"
                onClick={closeChat}
                title="Back to the chat list"
                aria-label="Back to the chat list"
              >
                <ArrowLeft size={15} />
              </button>
              <span className="dv-chat-title">{chat?.title ?? 'Chat'}</span>
              {chat?.running && (
                <span className="dv-live">
                  <span className="dv-dot" aria-hidden="true" />
                  reading
                </span>
              )}
              <span className="dv-spacer" />
              <span className="dv-chat-facts">
                {openRow?.head ?? chat?.head ?? 0} events
                {when(openRow?.updatedAt ?? chat?.updatedAt)
                  ? ` · ${when(openRow?.updatedAt ?? chat?.updatedAt)}`
                  : ''}
              </span>
            </div>
            <div className="dv-transcript">
              {loadingChat && !chat?.timeline?.length && (
                <p className="dv-note">Reading the log from the computer.</p>
              )}
              {chat?.timeline?.length === 0 && !loadingChat && !chatProblem && (
                <p className="dv-note">This chat has no messages this page can read yet.</p>
              )}
              {chat?.timeline?.map((item, i) => (
                <Item
                  key={item.id ?? `${item.kind}-${i}`}
                  item={item}
                  onAnswer={answer}
                  answers={answers}
                  answeringId={answeringId}
                  answerProblems={answerProblems}
                />
              ))}
              {chatProblem && <p className="dv-problem">{chatProblem}</p>}
            </div>
            <div className="dv-composer">
              {sendProblem && <p className="dv-problem">{sendProblem}</p>}
              <div className="dv-composer-box">
                <textarea
                  className="dv-input"
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey) {
                      e.preventDefault()
                      void send()
                    }
                  }}
                  placeholder="Message this chat"
                  rows={1}
                  disabled={closed}
                />
                <button
                  type="button"
                  className="dv-send"
                  onClick={() => send()}
                  disabled={!canSend(draft) || sending || closed}
                  title={sending ? 'Sending' : 'Send'}
                  aria-label={sending ? 'Sending' : 'Send'}
                >
                  <PaperPlaneRight size={15} weight="bold" />
                </button>
              </div>
              <p className="dv-hint">Enter to send, Shift+Enter for a new line</p>
            </div>
          </section>
        ) : (
          <div className="dv-home">
            <div className="dv-title-block">
              <h1 className="dv-page-title">Chats</h1>
              <p className="dv-page-sub" title={WHAT_THIS_IS}>
                From {machine.name}. Read every few seconds, not a live stream.
              </p>
            </div>
            {listProblem && (
              <p className="dv-problem dv-list-problem">
                {listProblem}
                <button type="button" className="dv-retry" onClick={() => loadSummary()}>
                  Try again
                </button>
              </p>
            )}
            {sessions.length === 0 && !listProblem && raw == null ? (
              <div className="dv-skeleton" aria-hidden="true">
                {[0, 1, 2].map((n) => (
                  <div className="dv-skeleton-row" key={n}>
                    <span className="dv-skeleton-title" />
                    <span className="dv-skeleton-line" />
                  </div>
                ))}
              </div>
            ) : sessions.length === 0 && !listProblem ? (
              <div className="dv-empty">
                <p className="dv-empty-title">No synced chats yet</p>
                <p className="dv-empty-line">{EMPTY_SUMMARY}</p>
              </div>
            ) : (
              <div className="dv-groups">
                {groups.map((group) => (
                  <section className="dv-group" key={group.label} aria-label={group.label}>
                    <h2 className="dv-group-label">{group.label}</h2>
                    <ul className="dv-rows">
                      {group.rows.map((row) => (
                        <li key={row.id}>
                          <button type="button" className="dv-row" onClick={() => openChat(row)}>
                            <span className="dv-row-body">
                              <span className="dv-row-name">{rowTitle(row, titles)}</span>
                              <span className="dv-row-line">
                                {rowLine(row, {
                                  when: (ms) => relativeWhen(ms),
                                  id: rowTitle(row, titles) === 'Untitled chat' ? shortId(row.id) : null,
                                })}
                              </span>
                            </span>
                            <CaretRight className="dv-row-caret" size={12} />
                          </button>
                        </li>
                      ))}
                    </ul>
                  </section>
                ))}
              </div>
            )}
          </div>
        )}
      </main>
    </div>
  )
}

// Folds daemon events into the chat model the UI renders. Pure: the state
// layer feeds it events in seq order and tests pin the shapes.
//
// The timeline is event order - the transcript is the truth, so every
// rendered item (message, tool, decision) lands in the rail exactly when
// the daemon said it happened.

import { displayWord, isActive } from '../status-words.js'

// The UI's dot vocabulary maps the daemon's session statuses. The words and
// the mapping live in status-words.js now, so the dot, the metrics sentence
// and the unread mark cannot disagree about what a status is called.
export function chatStatus(status) {
  return displayWord(status)
}

export function workspaceName(path) {
  const clean = String(path ?? '').replace(/[\\/]+$/, '')
  const base = clean.split(/[\\/]/).filter(Boolean).pop() ?? clean
  return base || 'workspace'
}

export function titleFrom(text) {
  const first = String(text ?? '').trim().split('\n')[0]
  return first.length > 44 ? `${first.slice(0, 44).trimEnd()}…` : first
}

/// The words a chat's generated name comes from: the first user message that
/// has any (a message of pictures alone names nothing), or null. A rename
/// left empty goes back to `titleFrom` of these.
export function firstPromptText(timeline) {
  for (const item of timeline ?? []) {
    if (item?.kind === 'message' && item.role === 'user' && item.text?.trim()) return item.text
  }
  return null
}

/// A fresh chat view over one daemon session (plus any known events).
export function chatFromSession(session) {
  const chat = {
    id: session.id,
    harness: session.harness,
    status: chatStatus(session.status),
    workspace: session.workspace,
    project: workspaceName(session.workspace),
    model: session.model ?? null,
    effort: session.effort ?? null,
    permissionMode: session.permission_mode ?? null,
    fast: session.fast ?? false,
    approvedTools: session.approved_tools ?? [],
    // The harness's own context numbers, when it reports them.
    context: null,
    // The session's running cost in USD - the harness's own numbers,
    // summed. Only where the harness reports money.
    costUsd: 0,
    // The session's running thinking-token count - the harness's own
    // number (claude's thinking_tokens), summed across turns.
    thinkingTokens: 0,
    lastError: session.last_error ?? null,
    running: isActive(session.status),
    // The harness has its own conversation for this chat (claude's
    // `--resume` id, codex's thread id, ...). Without one - a chat stopped
    // before its first message - there is nothing to resume, ever.
    resumable: Boolean(session.harness_session_ref),
    // The daemon keeps each chat's title, so every device shows the same
    // name before the transcript loads.
    title: session.title ?? null,
    // The session's own last-write stamp, in ms. A poll beat is a snapshot
    // taken at some moment: comparing stamps is how the merge knows
    // whether the name it carries is newer than the one on screen.
    updatedAt: session.updated_at ?? 0,
    // The device that runs a synced chat; none for a private one.
    executor: session.executor ?? null,
    // The chat whose agent started this one, when one did: this chat is a
    // worker in that chat's team ({ chat, depth, role, brief }).
    parent: session.parent ?? null,
    // Messages waiting for the running turn to end (the daemon's queue),
    // and whether an interrupt, a stop or a failure is holding them.
    queue: session.queue ?? [],
    queuePaused: Boolean(session.queue_paused) && (session.queue ?? []).length > 0,
    timeline: [],
    pendingDecisionId: null,
    // The highest event seq already folded into the timeline - reopening
    // a chat replays only from here, so the transcript never doubles.
    foldedSeq: null,
  }
  for (const event of session.events ?? []) {
    foldEvent(chat, event)
    if (event.seq != null) chat.foldedSeq = event.seq
  }
  return chat
}

/// Merge a polled session payload into an already-folded chat record.
/// The session owns the identity facts (status, workspace, model); the
/// folded transcript owns everything the payload doesn't carry. The
/// preserved list is the contract: a poll beat must never reset what
/// the stream folded - the timeline and its cursor, the pending
/// decision, the context numbers, the decisions list, and the running
/// cost and thinking-token totals (the session payload carries neither
/// total, so nothing else can restore them). The title is the one fact the
/// stamp decides: the daemon's name is the truth, but a beat taken before a
/// rename this console already folded must not take the newer name back.
///
/// A beat that changes nothing returns the record it was given. The poll
/// lands every 2.5 s, so a merge that always allocates would hand the
/// shell a new chat object on every beat and re-render the whole app while
/// a turn is streaming: identity here is the difference between "the beat
/// said the same thing" and "the beat said something new", and the views
/// are keyed on it. The check is on values, not on the payload, because a
/// fold can change a record between beats (a rename, a status event) and
/// the beat must still win in exactly the cases it won before.
export function mergeSessionIntoChat(existing, session) {
  const fresh = chatFromSession(session)
  // The session's last-write stamp says when this snapshot was taken, so a
  // beat that raced a rename loses to the rename. A beat carrying no title
  // at all (a session the daemon has not named yet) never blanks the name
  // the console derived from the first prompt.
  // `>=` on purpose: an equal stamp goes to the beat, which is the fresher
  // snapshot of the two, and the next beat settles it if they disagree.
  //
  // `foldedAt` is the newest event the fold has taken (`Event.at`, the daemon's own
  // millisecond stamp of when it happened), and it is what makes this comparison honest:
  // an event carries no session stamp, so measuring the payload against the record's
  // `updatedAt` alone measured it against the last beat, and a beat taken before the
  // event still won by equality. Now the beat wins only when its snapshot is at least as
  // new as every event the timeline already holds. A record with no fold behind it (a
  // first paint, a test fixture) has no `foldedAt` and compares exactly as before.
  const settled = Math.max(existing.updatedAt ?? 0, existing.foldedAt ?? 0)
  const beatIsNewer = (fresh.updatedAt ?? 0) >= settled
  const merged = {
    ...fresh,
    title: (beatIsNewer ? fresh.title : null) ?? existing.title,
    // The title was gated this way and these were not, so the same beat could put the
    // older answer back into the state, its reason, its liveness and its queue: a finished
    // turn flickered to running, the error line vanished for a beat.
    status: beatIsNewer ? fresh.status : existing.status,
    running: beatIsNewer ? fresh.running : existing.running,
    lastError: beatIsNewer ? fresh.lastError : existing.lastError,
    queue: beatIsNewer ? fresh.queue : existing.queue,
    updatedAt: Math.max(existing.updatedAt ?? 0, fresh.updatedAt ?? 0),
    // Kept through the merge: it is the record's memory of the newest event folded, and
    // the next beat is measured against it.
    foldedAt: existing.foldedAt,
    timeline: existing.timeline,
    foldedSeq: existing.foldedSeq,
    pendingDecisionId: existing.pendingDecisionId,
    context: existing.context,
    decisionsList: existing.decisionsList,
    costUsd: existing.costUsd ?? 0,
    thinkingTokens: existing.thinkingTokens ?? 0,
    // The parts behind those two totals: the baseline (what came before any
    // turn this record folded, including turn 0 payloads from an older daemon)
    // and one contribution per turn. A poll must keep them, or the next turn
    // event would recompute the total from nothing and lose the history.
    costBase: existing.costBase,
    costTurns: existing.costTurns,
    thinkingBase: existing.thinkingBase,
    thinkingTurns: existing.thinkingTurns,
  }
  // A beat that says the session stopped running accounts for a turn end
  // the stream never delivered: a lost event, a socket that died mid-turn.
  // The partial text is real and stays in the transcript; what cannot stay
  // is its liveness, a caret breathing under an idle dot. Only the
  // transition counts, because a beat taken while the chat was already
  // idle is no news about a turn.
  if (existing.running && !merged.running) {
    const timeline = [...existing.timeline]
    retireLiveItems({ timeline }, null)
    merged.timeline = timeline
  }
  return sameRecord(merged, existing) ? existing : merged
}

/// Whether two chat records carry the same values, which is what lets the
/// merge above hand back the old one. The comparison starts from identity,
/// which is free and covers every field the merge passes through by
/// reference (the transcript, the decisions, the context), then falls back
/// to values: arrays item by item, because the queue and the approved tools
/// are re-allocated from every payload, and plain objects key by key, for
/// the same reason. Only the keys of the new record are compared, so the
/// field set is the merge's: a field it stopped writing is not a change
/// until one of the keys it does write moves.
function sameRecord(next, existing) {
  for (const key of Object.keys(next)) {
    if (!sameValue(next[key], existing[key])) return false
  }
  return true
}

function sameValue(a, b) {
  if (Object.is(a, b)) return true
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false
    return a.every((item, i) => sameValue(item, b[i]))
  }
  if (a && b && typeof a === 'object' && typeof b === 'object') {
    const keys = Object.keys(a)
    if (keys.length !== Object.keys(b).length) return false
    return keys.every((key) => sameValue(a[key], b[key]))
  }
  return false
}

/// The chats whose sessions the polled list no longer carries - a chat
/// deleted on another device, whose pane and live stream would otherwise
/// stay pointed at a session the daemon answers 404 for. A local delete
/// already removed its own record, so this is the synced-delete path.
/// Only ids a poll once carried count: a chat created between beats is
/// live in the daemon but not yet in this list, and its record (from the
/// create response, fresher than the poll) must survive the stale beat.
export function vanishedChatIds(chats, sessions, polledIds) {
  const live = new Set(sessions.map((s) => s.id))
  const gone = []
  for (const id of Object.keys(chats)) {
    if (live.has(id) || !polledIds.has(id)) continue
    gone.push(id)
  }
  return gone
}

/// Retire the live items of a turn that is over. A settled message or
/// reasoning block normally replaces them, but a turn can end without one:
/// an interrupt, a stop, a harness that exited. The partial text is real
/// and stays in the transcript; only its liveness was a claim nobody can
/// still make, and a block that keeps a caret breathing says an idle chat
/// is still speaking. `turn` narrows the sweep to one turn; null is every
/// live item, for when the session itself is over.
function retireLiveItems(chat, turn) {
  for (let i = 0; i < chat.timeline.length; i++) {
    const item = chat.timeline[i]
    if (turn != null && item.turn !== turn) continue
    if (item.kind === 'stream') {
      // The settled shape message.added pushes, so nothing downstream has
      // to know this reply arrived in fragments.
      chat.timeline[i] = { kind: 'message', id: item.id, role: 'agent', text: item.text }
    } else if (item.kind === 'reasoning-stream') {
      chat.timeline[i] = { kind: 'reasoning', id: item.id, turn: item.turn, text: item.text }
    }
  }
}

/// The running total is the baseline plus one contribution per turn.
///
/// `kind` is 'cost' or 'thinking'. A payload that carries a turn owns its
/// contribution, including turn 0, which the daemon's first turn is: the same
/// turn re-reported under a new seq replaces what it said instead of adding
/// again. A payload with no turn (`null`, or absent from an older daemon) is
/// added to the baseline, exactly as every usage event was before the turn
/// existed. A record folded before the parts existed carries only its total,
/// which becomes the baseline on the first turn event so nothing already
/// counted is dropped. The published fields (`costUsd`, `thinkingTokens`) stay
/// the numbers the views read; the parts are internal to the record.
function addTurnTotal(chat, kind, turn, value) {
  const baseKey = kind === 'cost' ? 'costBase' : 'thinkingBase'
  const turnsKey = kind === 'cost' ? 'costTurns' : 'thinkingTurns'
  const totalKey = kind === 'cost' ? 'costUsd' : 'thinkingTokens'
  const base = Number.isFinite(chat[baseKey])
    ? chat[baseKey]
    : Number.isFinite(chat[totalKey])
      ? chat[totalKey]
      : 0
  const turns = { ...(chat[turnsKey] ?? {}) }
  // Presence of a turn, not its value: 0 is the first turn and is real, while
  // `null`/absent is a daemon that did not say.
  const real = Number.isInteger(turn)
  if (real) turns[turn] = value
  chat[baseKey] = real ? base : base + value
  chat[turnsKey] = turns
  chat[totalKey] = Object.values(turns).reduce((sum, n) => sum + n, chat[baseKey])
}

/// Fold one event into the chat. Events arrive in seq order; the timeline
/// preserves that order verbatim. `foldedSeq` advances with each fold -
/// the seq the timeline has already accounted for, so a replay (a chat
/// reopened) never doubles the transcript.
export function foldEvent(chat, event) {
  const payload = event
  // A replayed event - one at or below the fold cursor - is a duplicate:
  // the timeline already accounts for it. Folding is idempotent, so no
  // caller (a reopened chat, a reconnect) can double the transcript.
  if (event.seq != null && chat.foldedSeq != null && event.seq <= chat.foldedSeq) return chat
  switch (payload.type) {
    case 'session.created': {
      const session = payload.session
      chat.status = chatStatus(session.status)
      chat.workspace = session.workspace
      chat.project = workspaceName(session.workspace)
      break
    }
    case 'session.status_changed': {
      chat.status = chatStatus(payload.status)
      // The reason latches on failure, but only until the session is
      // healthy again: a non-failed status with no reason clears it, so
      // a later failure that carries no reason of its own never
      // resurfaces an old, unrelated failure's words.
      if (payload.reason != null) chat.lastError = payload.reason
      else if (payload.status !== 'failed') chat.lastError = null
      chat.running = isActive(payload.status)
      // A stop or a failure ends the turn with no settled message ever
      // coming; a turn that is working or waiting on an answer is still
      // speaking, and its live block stays honest.
      if (payload.status === 'failed' || payload.status === 'stopped') {
        retireLiveItems(chat, null)
      }
      break
    }
    case 'queue.changed': {
      chat.queue = payload.queue ?? []
      chat.queuePaused = Boolean(payload.paused) && chat.queue.length > 0
      break
    }
    case 'session.updated': {
      // Session facts outside the status lifecycle - a session-scoped tool
      // grant landing. The whole session rides along; fold the fields the
      // facts row renders.
      const session = payload.session
      chat.status = chatStatus(session.status)
      chat.running = isActive(session.status)
      // The same error latch as session.status_changed: set when the
      // payload carries one, keep on a failure without one, clear when
      // the session is healthy and reports no error.
      if (session.last_error != null) chat.lastError = session.last_error
      else if (session.status !== 'failed') chat.lastError = null
      // A rename arrives as this event with the whole session, so the name
      // the user asked for lands on the open chat now instead of on the
      // next beat. The stamp is what keeps a beat read before the rename
      // from undoing it.
      if (session.title != null) chat.title = session.title
      if (Number.isFinite(session.updated_at)) {
        chat.updatedAt = Math.max(chat.updatedAt ?? 0, session.updated_at)
      }
      if (session.model) chat.model = session.model
      if (session.effort) chat.effort = session.effort
      if (session.permission_mode) chat.permissionMode = session.permission_mode
      if (session.harness_session_ref) chat.resumable = true
      chat.fast = session.fast ?? chat.fast
      chat.approvedTools = session.approved_tools ?? chat.approvedTools
      break
    }
    case 'message.added': {
      const message = payload.message
      // The settled agent message replaces the stream item it settles -
      // the stream was its preview, never a second transcript entry.
      if (message.role === 'assistant') {
        for (let i = chat.timeline.length - 1; i >= 0; i--) {
          const item = chat.timeline[i]
          if (item.kind === 'stream') {
            if (item.turn === message.turn) chat.timeline.splice(i, 1)
            break
          }
        }
      }
      chat.timeline.push({
        kind: 'message',
        id: message.id,
        role: message.role === 'user' ? 'user' : 'agent',
        text: message.text,
        // Set when another agent sent it (a lead's brief, a worker's
        // report): { kind: 'agent', chat, title, report }.
        origin: message.origin ?? null,
        // The pictures that rode it - the user's, or the ones another agent
        // sent with its brief or report - as the blob store names them
        // ({ blob, mime }); the bytes are fetched when the message shows.
        images: Array.isArray(message.images) ? message.images : [],
      })
      // A message of pictures alone does not name the chat.
      if (message.role === 'user' && !chat.title && message.text?.trim()) chat.title = titleFrom(message.text)
      break
    }
    case 'message.delta': {
      // A streaming fragment of the agent's reply. It grows the live
      // streaming item; the final message.added replaces the whole thing,
      // so the transcript never holds both the stream and its settled form.
      let last = chat.timeline[chat.timeline.length - 1]
      if (!last || last.kind !== 'stream' || last.turn !== payload.turn) {
        last = { kind: 'stream', id: `stream-${payload.turn}`, turn: payload.turn, text: '' }
        chat.timeline.push(last)
        last.text = payload.text
        break
      }
      last.text += payload.text
      break
    }
    case 'tool.started': {
      chat.timeline.push({
        kind: 'tool',
        id: `tool-${event.seq}`,
        name: payload.name,
        // The turn the call belongs to - the wire carries it on both
        // tool.started and tool.result, so results pair within the
        // right turn instead of the nearest same-name tool anywhere.
        turn: payload.turn,
        input: payload.input,
        state: 'ok',
        result: null,
        images: [],
      })
      break
    }
    case 'context.used': {
      chat.context = { used: payload.used, window: payload.window ?? null }
      break
    }
    case 'usage.cost': {
      // The harness's own cost number for one turn, verbatim. The daemon says
      // which turn it belongs to (0 means an older daemon did not say), so a
      // turn re-reported under a new seq replaces its own contribution rather
      // than adding again; a turn 0 payload is added, as it always was. A
      // payload that is not a finite number is skipped, not folded: 0 +
      // undefined is NaN, and NaN would render as "$NaN" and survive every
      // merge.
      if (Number.isFinite(payload.cost_usd)) {
        addTurnTotal(chat, 'cost', payload.turn, payload.cost_usd)
      }
      break
    }
    case 'reasoning.added': {
      // The harness's own reasoning, settled - its own block, dim and
      // collapsible, never mixed into the reply text. Replaces the live
      // reasoning stream of the same turn.
      for (let i = chat.timeline.length - 1; i >= 0; i--) {
        const item = chat.timeline[i]
        if (item.kind === 'reasoning-stream') {
          if (item.turn === payload.turn) chat.timeline.splice(i, 1)
          break
        }
      }
      chat.timeline.push({
        kind: 'reasoning',
        id: `reasoning-${event.seq}`,
        turn: payload.turn,
        text: payload.text,
      })
      break
    }
    case 'reasoning.delta': {
      // A streaming fragment of the reasoning - grows the live item of
      // that turn; reasoning.added for the same turn replaces it whole.
      let last = chat.timeline[chat.timeline.length - 1]
      if (!last || last.kind !== 'reasoning-stream' || last.turn !== payload.turn) {
        last = { kind: 'reasoning-stream', id: `rstream-${payload.turn}`, turn: payload.turn, text: payload.text }
        chat.timeline.push(last)
        break
      }
      last.text += payload.text
      break
    }
    case 'thinking.tokens': {
      // The turn's thinking-token count, the harness's own number, with the
      // same rule as the cost: a real turn replaces its own contribution, a
      // turn 0 payload is added. Non-finite payloads are skipped for the same
      // reason as the cost total.
      if (Number.isFinite(payload.tokens)) {
        addTurnTotal(chat, 'thinking', payload.turn, payload.tokens)
      }
      break
    }
    case 'note.added': {
      chat.timeline.push({
        kind: 'message',
        id: `note-${event.seq}`,
        role: 'note',
        text: payload.text,
      })
      break
    }
    case 'tool.result': {
      // The result completes the most recent unanswered tool of that
      // name in that turn - never one of another turn, so a result
      // that lands after an interrupt cannot complete the next turn's
      // call. Two parallel same-name calls within one turn still need
      // a daemon-side call id to pair perfectly.
      for (let i = chat.timeline.length - 1; i >= 0; i--) {
        const item = chat.timeline[i]
        if (item.kind === 'tool' && item.turn === payload.turn && item.name === payload.name && item.result == null) {
          item.result = typeof payload.output === 'string' ? payload.output : JSON.stringify(payload.output)
          item.state = payload.is_error ? 'failed' : 'ok'
          // What the tool answered with besides words (an agent's
          // screenshot of a page), named in the blob store.
          item.images = Array.isArray(payload.images) ? payload.images : []
          break
        }
      }
      break
    }
    case 'decision.requested': {
      const decision = payload.decision
      chat.timeline.push({
        kind: 'decision',
        id: decision.id,
        toolName: decision.harness_request?.tool_name ?? null,
        input: decision.harness_request?.input ?? null,
        options: decision.options ?? [],
        pending: decision.state === 'pending',
        state: decision.state,
        answeredChoice: null,
      })
      if (decision.state === 'pending') chat.pendingDecisionId = decision.id
      break
    }
    case 'decision.responded': {
      for (let i = chat.timeline.length - 1; i >= 0; i--) {
        const item = chat.timeline[i]
        if (item.kind === 'decision' && item.id === payload.decision_id) {
          item.pending = false
          item.state = 'answered'
          item.answeredChoice = payload.choice
          break
        }
      }
      if (chat.pendingDecisionId === payload.decision_id) chat.pendingDecisionId = null
      break
    }
    case 'stream_gap': {
      // The transport skipped a frame it could not read and moved its cursor
      // past it, so this chat's history has a hole here. One note per run of
      // skips, in the shape a daemon note takes, because a gap the transcript
      // cannot show is a gap the person will not know about.
      chat.timeline.push({
        kind: 'message',
        id: `gap-${chat.timeline.length}`,
        role: 'note',
        text: 'One update from the daemon could not be read, so this chat is missing whatever it said.',
      })
      break
    }
    case 'daemon.error': {
      chat.timeline.push({
        kind: 'message',
        // `id` when the console wrote this itself (a refusal the stream could not
        // survive); the daemon's own events carry a seq instead.
        id: `daemon-${event.id ?? event.seq}`,
        role: 'note',
        text: payload.message,
      })
      break
    }
    case 'turn.started':
      break
    case 'turn.completed':
      // A turn that succeeded was settled by its message.added, so there is
      // nothing left to retire. One that was interrupted or failed was not,
      // and the partial reply is all the user is left with.
      retireLiveItems(chat, payload.turn ?? null)
      break
    case 'message.delta':
    default:
      break
  }
  // Folded, so only now does the timeline own the seq. Set before the switch, a payload
  // that threw while folding consumed its own event: the record claimed a seq the
  // timeline never received, and every replay - a reopen, a reconnect resuming from this
  // cursor - skipped it, leaving a hole in the transcript that nothing ever mentioned.
  if (event.seq != null) chat.foldedSeq = event.seq
  // When it happened, by the daemon's own clock, so the next poll beat can tell whether
  // its snapshot predates this event. Same millisecond unit as the session's
  // `updated_at`, which is what makes the comparison meaningful.
  if (event.at != null) chat.foldedAt = Math.max(chat.foldedAt ?? 0, event.at)
  return chat
}

/// Fold a batch of events in one pass. The caller copies the chat and its
/// timeline once for the batch and publishes the result, so a burst of events
/// costs one copy rather than one per event; the fold itself is the same
/// function called in the same order, so the timeline, the statuses and the
/// counters are the ones per-event folding produced. reducer.test.js folds one
/// recorded stream both ways and compares the results.
export function foldEvents(chat, events) {
  for (const event of events ?? []) {
    // An event this fold cannot read is a gap, not a crash and not a redial loop. Before
    // this guard a malformed payload threw out of the whole batch, and because the cursor
    // only advances past a successful fold, every retry re-delivered the same event: the
    // pane failed again on each attempt with nothing in the transcript saying why.
    try {
      foldEvent(chat, event)
    } catch {
      chat.timeline.push({
        kind: 'message',
        id: `gap-${chat.timeline.length}`,
        role: 'note',
        text: 'One update from the daemon could not be read, so this chat is missing whatever it said.',
      })
      if (event?.seq != null) chat.foldedSeq = event.seq
    }
  }
  return chat
}

/// The rail the ChatView renders - the timeline itself, in event order.
export function railItems(chat) {
  return chat.timeline
}

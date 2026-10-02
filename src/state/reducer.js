// Folds daemon events into the chat model the UI renders. Pure: the state
// layer feeds it events in seq order and tests pin the shapes.
//
// The timeline is event order — the transcript is the truth, so every
// rendered item (message, tool, decision) lands in the rail exactly when
// the daemon said it happened.

const STATUS = {
  starting: 'starting',
  working: 'running',
  waiting: 'waiting',
  idle: 'idle',
  stopped: 'stopped',
  failed: 'failed',
}

// The UI's dot vocabulary maps the daemon's session statuses.
export function chatStatus(status) {
  return STATUS[status] ?? 'idle'
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
    fast: session.fast ?? false,
    approvedTools: session.approved_tools ?? [],
    // The harness's own context numbers, when it reports them.
    context: null,
    lastError: session.last_error ?? null,
    running: ['starting', 'working', 'waiting'].includes(session.status),
    title: null,
    timeline: [],
    pendingDecisionId: null,
    // The highest event seq already folded into the timeline — reopening
    // a chat replays only from here, so the transcript never doubles.
    foldedSeq: null,
  }
  for (const event of session.events ?? []) {
    foldEvent(chat, event)
    if (event.seq != null) chat.foldedSeq = event.seq
  }
  return chat
}

/// Fold one event into the chat. Events arrive in seq order; the timeline
/// preserves that order verbatim. `foldedSeq` advances with each fold —
/// the seq the timeline has already accounted for, so a replay (a chat
/// reopened) never doubles the transcript.
export function foldEvent(chat, event) {
  const payload = event
  // A replayed event — one at or below the fold cursor — is a duplicate:
  // the timeline already accounts for it. Folding is idempotent, so no
  // caller (a reopened chat, a reconnect) can double the transcript.
  if (event.seq != null) {
    if (chat.foldedSeq != null && event.seq <= chat.foldedSeq) return chat
    chat.foldedSeq = event.seq
  }
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
      chat.lastError = payload.reason ?? chat.lastError
      chat.running = ['starting', 'working', 'waiting'].includes(payload.status)
      break
    }
    case 'session.updated': {
      // Session facts outside the status lifecycle — a session-scoped tool
      // grant landing. The whole session rides along; fold the fields the
      // facts row renders.
      const session = payload.session
      chat.status = chatStatus(session.status)
      chat.running = ['starting', 'working', 'waiting'].includes(session.status)
      chat.lastError = session.last_error ?? chat.lastError
      if (session.model) chat.model = session.model
      if (session.effort) chat.effort = session.effort
      chat.fast = session.fast ?? chat.fast
      chat.approvedTools = session.approved_tools ?? chat.approvedTools
      break
    }
    case 'message.added': {
      const message = payload.message
      chat.timeline.push({
        kind: 'message',
        id: message.id,
        role: message.role === 'user' ? 'user' : 'agent',
        text: message.text,
      })
      if (message.role === 'user' && !chat.title) chat.title = titleFrom(message.text)
      break
    }
    case 'tool.started': {
      chat.timeline.push({
        kind: 'tool',
        id: `tool-${event.seq}`,
        name: payload.name,
        input: payload.input,
        state: 'ok',
        result: null,
      })
      break
    }
    case 'context.used': {
      chat.context = { used: payload.used, window: payload.window ?? null }
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
      // The result completes the most recent unanswered tool of that name.
      for (let i = chat.timeline.length - 1; i >= 0; i--) {
        const item = chat.timeline[i]
        if (item.kind === 'tool' && item.name === payload.name && item.result == null) {
          item.result = typeof payload.output === 'string' ? payload.output : JSON.stringify(payload.output)
          item.state = payload.is_error ? 'failed' : 'ok'
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
    case 'daemon.error': {
      chat.timeline.push({
        kind: 'message',
        id: `daemon-${event.seq}`,
        role: 'note',
        text: payload.message,
      })
      break
    }
    case 'turn.started':
    case 'turn.completed':
    case 'message.delta':
    default:
      break
  }
  return chat
}

/// The rail the ChatView renders — the timeline itself, in event order.
export function railItems(chat) {
  return chat.timeline
}

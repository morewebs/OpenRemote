import assert from 'node:assert/strict'
import test from 'node:test'
import { chatFromSession, chatStatus, foldEvent, foldEvents, mergeSessionIntoChat, railItems, titleFrom, vanishedChatIds, workspaceName } from './reducer.js'

const session = (over = {}) => ({
  id: 's1',
  harness: 'claude',
  workspace: 'C:\\dev\\webapp',
  status: 'idle',
  last_error: null,
  ...over,
})

const event = (seq, payload) => ({ seq, session_id: 's1', ...payload })

test('a session folds into a chat with workspace project and status', () => {
  const chat = chatFromSession(session({ status: 'working' }))
  assert.equal(chat.status, 'running')
  assert.equal(chat.project, 'webapp')
  assert.equal(chat.running, true)
  assert.equal(workspaceName('~/src/api/'), 'api')
  assert.equal(chatStatus('waiting'), 'waiting')
})

test('the timeline preserves event order: user, tools, decision, answer, agent', () => {
  let chat = chatFromSession(session())
  const events = [
    event(0, { type: 'session.created', session: session() }),
    event(1, { type: 'session.status_changed', status: 'working', reason: null }),
    event(2, {
      type: 'message.added',
      message: { id: 'm1', turn: 0, role: 'user', text: 'fix the failing test' },
    }),
    event(3, { type: 'tool.started', turn: 0, name: 'Bash', input: { command: 'npm test' } }),
    event(4, {
      type: 'decision.requested',
      decision: {
        id: 'd1',
        state: 'pending',
        kind: 'approval',
        harness_request: { tool_name: 'Bash', input: { command: 'npm test' } },
        options: [
          { id: 'allow', label: 'Allow' },
          { id: 'deny', label: 'Deny' },
        ],
      },
    }),
    event(5, { type: 'decision.responded', decision_id: 'd1', choice: 'allow' }),
    event(6, { type: 'tool.result', turn: 0, name: 'Bash', output: '2 passed', is_error: false }),
    event(7, {
      type: 'message.added',
      message: { id: 'm2', turn: 0, role: 'assistant', text: 'The race is fixed.' },
    }),
    event(8, { type: 'turn.completed', turn: 0, outcome: 'success', coarse: 'completed' }),
    event(9, { type: 'session.status_changed', status: 'idle', reason: null }),
  ]
  for (const e of events) chat = foldEvent(chat, e)

  const rail = railItems(chat)
  assert.equal(rail.length, 4)
  assert.equal(rail[0].kind, 'message')
  assert.equal(rail[0].role, 'user')
  assert.equal(rail[1].kind, 'tool')
  assert.equal(rail[1].name, 'Bash')
  assert.equal(rail[1].result, '2 passed')
  assert.equal(rail[2].kind, 'decision')
  assert.equal(rail[2].pending, false)
  assert.equal(rail[2].answeredChoice, 'allow')
  assert.equal(rail[3].kind, 'message')
  assert.equal(rail[3].role, 'agent')
  assert.equal(rail[3].text, 'The race is fixed.')
  assert.equal(chat.title, 'fix the failing test')
  assert.equal(chat.status, 'idle')
  assert.equal(chat.pendingDecisionId, null)
})

test('a pending decision marks the chat as waiting on the operator', () => {
  let chat = chatFromSession(session())
  chat = foldEvent(
    chat,
    event(4, {
      type: 'decision.requested',
      decision: {
        id: 'd2',
        state: 'pending',
        kind: 'approval',
        harness_request: { tool_name: 'Bash', input: { command: 'rm -rf' } },
        options: [
          { id: 'allow', label: 'Allow' },
          { id: 'deny', label: 'Deny' },
        ],
      },
    }),
  )
  const rail = railItems(chat)
  assert.equal(rail.at(-1).pending, true)
  assert.equal(rail.at(-1).toolName, 'Bash')
  assert.deepEqual(
    rail.at(-1).options.map((o) => o.id),
    ['allow', 'deny'],
  )
  assert.equal(chat.pendingDecisionId, 'd2')
})

test('a failed session keeps its reason and the title truncates', () => {
  let chat = chatFromSession(session())
  chat = foldEvent(chat, event(1, { type: 'session.status_changed', status: 'failed', reason: 'harness process exited' }))
  assert.equal(chat.status, 'failed')
  assert.equal(chat.running, false)
  assert.equal(chat.lastError, 'harness process exited')
  assert.equal(titleFrom('a'.repeat(60)).length, 45)
  assert.ok(titleFrom('a'.repeat(60)).endsWith('…'))
})

test('daemon errors land as notes in the transcript', () => {
  let chat = chatFromSession(session())
  chat = foldEvent(chat, event(7, { type: 'daemon.error', message: 'boom' }))
  const rail = railItems(chat)
  assert.equal(rail.at(-1).role, 'note')
  assert.equal(rail.at(-1).text, 'boom')
})

test('a session carries its fast mode and approved tools from the daemon', () => {
  const chat = chatFromSession(session({ fast: true, approved_tools: ['commandExecution'] }))
  assert.equal(chat.fast, true)
  assert.deepEqual(chat.approvedTools, ['commandExecution'])
  // old sessions (and harnesses without a fast mode) fold to the defaults
  const plain = chatFromSession(session())
  assert.equal(plain.fast, false)
  assert.deepEqual(plain.approvedTools, [])
})

test('session.updated folds the session-scoped grant into the chat', () => {
  let chat = chatFromSession(session())
  chat = foldEvent(
    chat,
    event(8, {
      type: 'session.updated',
      session: session({ status: 'waiting', fast: true, approved_tools: ['commandExecution'] }),
    }),
  )
  assert.equal(chat.fast, true)
  assert.deepEqual(chat.approvedTools, ['commandExecution'])
  assert.equal(chat.status, 'waiting')
  assert.equal(chat.running, true)
})

test('the permission word rides the session both ways', () => {
  // A session created with the harness's own word carries it from the
  // start; a stopped chat's settings change folds through session.updated.
  const chat = chatFromSession(session({ permission_mode: 'bypassPermissions' }))
  assert.equal(chat.permissionMode, 'bypassPermissions')
  let changed = chatFromSession(session())
  changed = foldEvent(
    changed,
    event(8, {
      type: 'session.updated',
      session: session({ status: 'stopped', permission_mode: 'plan' }),
    }),
  )
  assert.equal(changed.permissionMode, 'plan')
  // Old sessions and wordless harnesses fold to null - the chip never
  // renders a word the harness didn't say.
  assert.equal(chatFromSession(session()).permissionMode, null)
})

test('resumable follows the harness conversation: present, absent, arrived late', () => {
  // A chat that ran a turn has the harness's own conversation to resume.
  assert.equal(chatFromSession(session({ harness_session_ref: 'c-42' })).resumable, true)
  // One stopped before its first message never got one - nothing to
  // resume, ever, and the console must not offer it.
  assert.equal(chatFromSession(session()).resumable, false)
  // The ref rides the first turn's init; the session.updated that follows
  // turns a dead chat resumable without a reload.
  let chat = chatFromSession(session())
  chat = foldEvent(
    chat,
    event(9, { type: 'session.updated', session: session({ harness_session_ref: 'c-42' }) }),
  )
  assert.equal(chat.resumable, true)
})

test('the harness\'s own context numbers fold in, and compaction notes land in order', () => {
  let chat = chatFromSession(session())
  chat = foldEvent(chat, event(9, { type: 'context.used', used: 1250, window: 200000 }))
  assert.deepEqual(chat.context, { used: 1250, window: 200000 })
  // a harness that reports no window leaves it unset
  chat = foldEvent(chat, event(10, { type: 'context.used', used: 40 }))
  assert.deepEqual(chat.context, { used: 40, window: null })
  // the compaction marker becomes a transcript note, in event order
  chat = foldEvent(
    chat,
    event(11, { type: 'note.added', text: 'This session was compacted before this message.' }),
  )
  const rail = railItems(chat)
  assert.equal(rail.at(-1).role, 'note')
  assert.match(rail.at(-1).text, /compacted/)
})

test('a chat with embedded events carries its folded seq, and a replay never doubles the transcript', () => {
  // A session fetched with its events folds them once.
  const withEvents = session({
    events: [
      event(2, { type: 'message.added', message: { id: 'm1', turn: 1, role: 'user', text: 'do the thing' } }),
      event(3, { type: 'message.added', message: { id: 'm2', turn: 1, role: 'assistant', text: 'done' } }),
    ],
  })
  const chat = chatFromSession(withEvents)
  assert.equal(chat.foldedSeq, 3)
  assert.equal(railItems(chat).length, 2)

  // The fold itself advances the seq - any path that folds without the
  // console's guard still cannot double: a replayed seq never regresses.
  const folded = foldEvent(chat, event(3, { type: 'message.added', message: { id: 'm2', turn: 1, role: 'assistant', text: 'done' } }))
  assert.equal(folded.foldedSeq, 3, 'a replayed seq does not regress the fold cursor')

  // A fresh event past the seq folds in and advances it.
  const next = foldEvent(folded, event(4, { type: 'message.added', message: { id: 'm3', turn: 2, role: 'user', text: 'again' } }))
  assert.equal(next.timeline.length, 3)
  assert.equal(next.foldedSeq, 4)
})

test('stream deltas grow a live item, and the settled message replaces it whole', () => {
  const chat = chatFromSession(session())
  // Three fragments of the agent's reply, then the settled message.
  foldEvent(chat, event(2, { type: 'turn.started', turn: 1 }))
  foldEvent(chat, event(3, { type: 'message.delta', turn: 1, text: 'Let me ' }))
  foldEvent(chat, event(4, { type: 'message.delta', turn: 1, text: 'check ' }))
  foldEvent(chat, event(5, { type: 'message.delta', turn: 1, text: 'that file.' }))
  let rail = railItems(chat)
  assert.equal(rail.length, 1, 'one live stream item grows')
  assert.equal(rail[0].kind, 'stream')
  assert.equal(rail[0].text, 'Let me check that file.')

  // The settled assistant message replaces the stream - never both.
  foldEvent(chat, event(6, {
    type: 'message.added',
    message: { id: 'm2', turn: 1, role: 'assistant', text: 'Let me check that file.' },
  }))
  rail = railItems(chat)
  assert.equal(rail.length, 1, 'the settled message replaces the stream item')
  assert.equal(rail[0].kind, 'message')
  assert.equal(rail[0].text, 'Let me check that file.')

  // A second turn streams into its own item, the first stays settled.
  foldEvent(chat, event(7, { type: 'turn.started', turn: 2 }))
  foldEvent(chat, event(8, { type: 'message.delta', turn: 2, text: 'Done. ' }))
  rail = railItems(chat)
  assert.equal(rail.length, 2)
  assert.equal(rail.at(-1).kind, 'stream')
  assert.equal(rail.at(-1).text, 'Done. ')
})

test('reasoning streams and settles as its own block, thinking tokens sum', () => {
  const chat = chatFromSession(session())
  // The thinking stream grows its own live item.
  foldEvent(chat, event(2, { type: 'reasoning.delta', turn: 1, text: 'Let me ' }))
  foldEvent(chat, event(3, { type: 'reasoning.delta', turn: 1, text: 'compute.' }))
  let rail = railItems(chat)
  assert.equal(rail.length, 1)
  assert.equal(rail[0].kind, 'reasoning-stream')
  assert.equal(rail[0].text, 'Let me compute.')

  // The settled thinking block replaces the stream.
  foldEvent(chat, event(4, { type: 'reasoning.added', turn: 1, text: 'Let me compute.' }))
  rail = railItems(chat)
  assert.equal(rail.length, 1)
  assert.equal(rail[0].kind, 'reasoning')
  assert.equal(rail[0].text, 'Let me compute.')

  // The reply lands unmixed, after the reasoning.
  foldEvent(chat, event(5, {
    type: 'message.added',
    message: { id: 'm1', turn: 1, role: 'assistant', text: '391' },
  }))
  rail = railItems(chat)
  assert.equal(rail.length, 2)
  assert.equal(rail[0].kind, 'reasoning')
  assert.equal(rail[1].text, '391')

  // Thinking tokens sum across turns.
  foldEvent(chat, event(6, { type: 'thinking.tokens', tokens: 42 }))
  foldEvent(chat, event(7, { type: 'thinking.tokens', tokens: 8 }))
  assert.equal(chat.thinkingTokens, 50)
})

test('a poll merge preserves the folded transcript, its cursor, and the running totals', () => {
  let chat = chatFromSession(session({ status: 'working' }))
  foldEvent(chat, event(2, { type: 'message.added', message: { id: 'm1', turn: 1, role: 'user', text: 'do the thing' } }))
  foldEvent(chat, event(3, { type: 'usage.cost', cost_usd: 0.42 }))
  foldEvent(chat, event(4, { type: 'thinking.tokens', tokens: 1234 }))
  foldEvent(chat, event(5, { type: 'context.used', used: 1250, window: 200000 }))
  foldEvent(chat, event(6, {
    type: 'decision.requested',
    decision: { id: 'd9', state: 'pending', kind: 'approval', harness_request: { tool_name: 'Bash' }, options: [] },
  }))
  // The stream's decisions list is console-side, never folded - set it
  // the way the stream does before a beat lands.
  chat.decisionsList = [{ id: 'd9', state: 'pending' }]

  // The polled payload carries none of what the stream folded (the
  // session payload has no cost or thinking numbers), so the merge
  // must keep every folded fact or the next beat resets it.
  const merged = mergeSessionIntoChat(chat, session({ status: 'idle' }))
  assert.equal(merged.title, 'do the thing')
  assert.equal(merged.foldedSeq, 6, 'the fold cursor survives the beat')
  assert.equal(merged.timeline.length, 2, 'the transcript survives the beat')
  assert.deepEqual(merged.context, { used: 1250, window: 200000 })
  assert.equal(merged.costUsd, 0.42, 'the running cost survives the beat')
  assert.equal(merged.thinkingTokens, 1234, 'the thinking-token total survives the beat')
  assert.equal(merged.pendingDecisionId, 'd9')
  assert.deepEqual(merged.decisionsList, [{ id: 'd9', state: 'pending' }])
  // The identity facts still follow the polled session.
  assert.equal(merged.status, 'idle')
  assert.equal(merged.running, false)

  // Events folded after the merge add to the preserved totals.
  foldEvent(merged, event(7, { type: 'usage.cost', cost_usd: 0.08 }))
  assert.equal(merged.costUsd, 0.5)

  // A chat that never folded a total merges to zero, not undefined.
  const bare = mergeSessionIntoChat(chatFromSession(session()), session())
  assert.equal(bare.costUsd, 0)
  assert.equal(bare.thinkingTokens, 0)
})

test('a beat that says the turn ended retires what the stream left live', () => {
  // The daemon says the session stopped, and the stream never delivered the
  // turn's end - a lost event, a socket that died mid-turn. The partial
  // reply is real and stays in the transcript; what cannot stay is its
  // liveness, a breathing caret under an idle dot.
  const chat = chatFromSession(session({ status: 'working' }))
  foldEvent(chat, event(1, { type: 'message.delta', turn: 1, text: 'half an ans' }))
  assert.equal(chat.timeline.at(-1).kind, 'stream')

  const merged = mergeSessionIntoChat(chat, session({ status: 'idle' }))
  assert.equal(merged.status, 'idle')
  assert.equal(merged.timeline.at(-1).kind, 'message', 'the beat settles the live block')
  assert.equal(merged.timeline.at(-1).text, 'half an ans', 'the partial text survives')
  // The fold is not edited behind its back: the merge returns a new record.
  assert.equal(chat.timeline.at(-1).kind, 'stream')

  // A failure ends a turn the same way, so the same retirement applies.
  const failed = mergeSessionIntoChat(chat, session({ status: 'failed', last_error: 'harness exited' }))
  assert.equal(failed.timeline.at(-1).kind, 'message')

  // A chat that was already idle brings no news about a turn: its timeline
  // comes through untouched, item for item.
  const idle = chatFromSession(session({ status: 'idle' }))
  foldEvent(idle, event(1, { type: 'message.added', message: { id: 'm1', turn: 1, role: 'agent', text: 'done' } }))
  const same = mergeSessionIntoChat(idle, session({ status: 'idle' }))
  assert.deepEqual(same.timeline, idle.timeline)
})

test('vanished ids: only a chat a poll once carried and then lost counts as gone', () => {
  const chats = {
    s1: chatFromSession(session({ id: 's1' })),
    s2: chatFromSession(session({ id: 's2' })),
    // Fresh from the create response - the next poll beat has not listed
    // it yet. Its record is newer than the list; it is not gone.
    s3: chatFromSession(session({ id: 's3' })),
  }
  // Both s1 and s2 once appeared in a beat; this beat dropped s2 (a
  // delete from another device).
  const polled = new Set(['s1', 's2'])
  const beat = [session({ id: 's1' })]
  assert.deepEqual(vanishedChatIds(chats, beat, polled), ['s2'])
  // The next beat re-adds nothing: gone stays gone (the record was
  // pruned), and the fresh chat survives its first no-show because no
  // beat ever carried it. Once its id lands in a beat, a later drop
  // would count - the same synced-delete contract.
  assert.deepEqual(vanishedChatIds({ s1: chats.s1, s3: chats.s3 }, beat, polled), [])
  const beatWithFresh = [session({ id: 's1' }), session({ id: 's3' })]
  const polledAfter = new Set(['s1', 's2', 's3'])
  assert.deepEqual(vanishedChatIds({ s1: chats.s1, s3: chats.s3 }, beatWithFresh, polledAfter), [])
})

test('parallel same-name tools in one turn pair, out-of-order completion included', () => {
  let chat = chatFromSession(session())
  // Two Bash calls in one turn, started in order A then B.
  foldEvent(chat, event(2, { type: 'tool.started', turn: 1, name: 'Bash', input: { command: 'npm test' } }))
  foldEvent(chat, event(3, { type: 'tool.started', turn: 1, name: 'Bash', input: { command: 'ls -la' } }))
  // B completes first - out of order. The most recent unanswered Bash of
  // that turn is B, so B's result lands on B and A stays open.
  foldEvent(chat, event(4, { type: 'tool.result', turn: 1, name: 'Bash', output: 'B ran', is_error: false }))
  let rail = railItems(chat)
  assert.equal(rail[0].result, null, 'A is still unanswered')
  assert.equal(rail[1].result, 'B ran')
  // A completes last and lands on the only unanswered Bash of that turn.
  foldEvent(chat, event(5, { type: 'tool.result', turn: 1, name: 'Bash', output: 'A ran', is_error: false }))
  rail = railItems(chat)
  assert.equal(rail[0].result, 'A ran')
  assert.equal(rail[1].result, 'B ran')
})

test('a late result never completes another turn\'s same-name tool', () => {
  let chat = chatFromSession(session())
  // Turn 1 is interrupted mid-tool: its Bash never got its result before
  // turn 2 starts the same tool.
  foldEvent(chat, event(2, { type: 'tool.started', turn: 1, name: 'Bash', input: { command: 'npm test' } }))
  foldEvent(chat, event(3, { type: 'tool.started', turn: 2, name: 'Bash', input: { command: 'ls -la' } }))
  // Turn 1's result arrives after turn 2's call started. Name-only
  // pairing would fold it onto turn 2's tool; turn scoping keeps
  // every result on its own turn's call.
  foldEvent(chat, event(4, { type: 'tool.result', turn: 1, name: 'Bash', output: 'turn 1 output', is_error: false }))
  let rail = railItems(chat)
  assert.equal(rail[0].result, 'turn 1 output')
  assert.equal(rail[1].result, null, 'turn 2\'s tool is untouched')
  // Turn 2's own result then completes its own tool.
  foldEvent(chat, event(5, { type: 'tool.result', turn: 2, name: 'Bash', output: 'turn 2 output', is_error: false }))
  rail = railItems(chat)
  assert.equal(rail[0].result, 'turn 1 output')
  assert.equal(rail[1].result, 'turn 2 output')
})

test('a failed chat\'s reason clears when the session is healthy again', () => {
  let chat = chatFromSession(session())
  chat = foldEvent(chat, event(1, { type: 'session.status_changed', status: 'failed', reason: 'harness process exited' }))
  assert.equal(chat.lastError, 'harness process exited')
  // The resume is a non-failed status with no reason: the latch clears.
  chat = foldEvent(chat, event(2, { type: 'session.status_changed', status: 'working', reason: null }))
  assert.equal(chat.lastError, null)
  // A later failure with no reason of its own shows nothing - not the
  // day-old text of the failure before it.
  chat = foldEvent(chat, event(3, { type: 'session.status_changed', status: 'failed', reason: null }))
  assert.equal(chat.lastError, null)
  // A failure that carries its own reason still latches.
  chat = foldEvent(chat, event(4, { type: 'session.status_changed', status: 'failed', reason: 'exited again' }))
  assert.equal(chat.lastError, 'exited again')
})

test('session.updated clears the error latch the same way', () => {
  let chat = chatFromSession(session())
  chat = foldEvent(chat, event(1, { type: 'session.status_changed', status: 'failed', reason: 'harness process exited' }))
  // A settings update riding a healthy, error-less session clears the
  // stale reason instead of keeping it forever.
  chat = foldEvent(chat, event(2, { type: 'session.updated', session: session({ status: 'working' }) }))
  assert.equal(chat.lastError, null)
  // A failed session payload carries its error and latches it.
  chat = foldEvent(chat, event(3, { type: 'session.updated', session: session({ status: 'failed', last_error: 'exited' }) }))
  assert.equal(chat.lastError, 'exited')
})

test('non-finite cost and token payloads never poison the running totals', () => {
  let chat = chatFromSession(session())
  // A missing field, a string, NaN, Infinity - none of them fold;
  // 0 + undefined would render "$NaN" and survive every merge.
  foldEvent(chat, event(2, { type: 'usage.cost' }))
  foldEvent(chat, event(3, { type: 'usage.cost', cost_usd: 'free' }))
  foldEvent(chat, event(4, { type: 'usage.cost', cost_usd: NaN }))
  foldEvent(chat, event(5, { type: 'thinking.tokens' }))
  foldEvent(chat, event(6, { type: 'thinking.tokens', tokens: Infinity }))
  assert.equal(chat.costUsd, 0)
  assert.equal(chat.thinkingTokens, 0)
  // A real number after the malformed ones still folds.
  foldEvent(chat, event(7, { type: 'usage.cost', cost_usd: 0.42 }))
  foldEvent(chat, event(8, { type: 'thinking.tokens', tokens: 42 }))
  assert.equal(chat.costUsd, 0.42)
  assert.equal(chat.thinkingTokens, 42)
})

test('the queue folds from the session and from queue.changed, and an empty one is never held', () => {
  const chat = chatFromSession({
    id: 's1',
    harness: 'claude',
    status: 'working',
    queue: [{ id: 'q1', text: 'second', at: 1 }],
    queue_paused: false,
  })
  assert.deepEqual(chat.queue.map((m) => m.text), ['second'])
  assert.equal(chat.queuePaused, false)
  foldEvent(chat, {
    seq: 1,
    type: 'queue.changed',
    queue: [{ id: 'q1', text: 'second', at: 1 }],
    paused: true,
  })
  assert.equal(chat.queuePaused, true)
  foldEvent(chat, { seq: 2, type: 'queue.changed', queue: [], paused: true })
  assert.deepEqual(chat.queue, [])
  assert.equal(chat.queuePaused, false)
  // A session.updated never rewinds the queue.
  foldEvent(chat, { seq: 3, type: 'queue.changed', queue: [{ id: 'q2', text: 'third', at: 2 }], paused: false })
  foldEvent(chat, { seq: 4, type: 'session.updated', session: { id: 's1', harness: 'claude', status: 'working' } })
  assert.deepEqual(chat.queue.map((m) => m.text), ['third'])
})

test('a rename lands live, and a beat snapshotted before it never takes the name back', () => {
  let chat = chatFromSession(session({ status: 'working', updated_at: 1000 }))
  foldEvent(chat, event(2, { type: 'message.added', message: { id: 'm1', turn: 1, role: 'user', text: 'do the thing' } }))
  // A fresh chat is never anonymous: the console names it from the first
  // prompt before the daemon has written a title of its own.
  assert.equal(chat.title, 'do the thing')

  // /rename stores the title, and the daemon emits the whole session.
  foldEvent(chat, event(3, { type: 'session.updated', session: session({ status: 'working', title: 'Release triage', updated_at: 2000 }) }))
  assert.equal(chat.title, 'Release triage', 'the rename folds into the open chat')

  // The poll beat read the session before the rename wrote it, and its
  // answer lands after: the older snapshot must not take the name back.
  const stale = mergeSessionIntoChat(chat, session({ status: 'working', title: 'do the thing', updated_at: 1500 }))
  assert.equal(stale.title, 'Release triage', 'a beat taken before the rename keeps the new name')
  assert.equal(stale.updatedAt, 2000, 'the record keeps the newest session stamp it has seen')

  // The next beat is newer than the rename and agrees with it.
  const next = mergeSessionIntoChat(stale, session({ status: 'idle', title: 'Release triage', updated_at: 2400 }))
  assert.equal(next.title, 'Release triage')
  assert.equal(next.updatedAt, 2400)
})

test('a rename made on another device reaches a chat this console is not streaming', () => {
  // With no live stream the poll beat is the only witness, so the
  // payload's newer name has to land.
  const record = chatFromSession(session({ status: 'idle', title: 'do the thing', updated_at: 1000 }))
  const merged = mergeSessionIntoChat(record, session({ status: 'idle', title: 'Renamed elsewhere', updated_at: 5000 }))
  assert.equal(merged.title, 'Renamed elsewhere')
})

test('a beat that carries no title keeps the one derived from the first prompt', () => {
  // The daemon names a session from its first prompt too, but a session
  // created before that write reports none: the console's own name is all
  // there is, and a beat must not blank it.
  let chat = chatFromSession(session({ status: 'working', updated_at: 1000 }))
  foldEvent(chat, event(2, { type: 'message.added', message: { id: 'm1', turn: 1, role: 'user', text: 'do the thing' } }))
  const merged = mergeSessionIntoChat(chat, session({ status: 'working', updated_at: 1200 }))
  assert.equal(merged.title, 'do the thing')
})

test('a turn that ends without its settled message retires the live reply', () => {
  let chat = chatFromSession(session({ status: 'working' }))
  foldEvent(chat, event(2, { type: 'message.added', message: { id: 'm1', turn: 1, role: 'user', text: 'count them' } }))
  foldEvent(chat, event(3, { type: 'reasoning.delta', turn: 1, text: 'counting' }))
  foldEvent(chat, event(4, { type: 'message.delta', turn: 1, text: 'There are ' }))
  assert.equal(railItems(chat).at(-1).kind, 'stream')

  // Esc. The harness never sends the assistant message that would have
  // settled the stream, so the partial text is all there is - and a live
  // block would keep a caret breathing in an idle chat.
  foldEvent(chat, event(5, { type: 'turn.completed', turn: 1, outcome: 'interrupted', coarse: 'stopped' }))
  const rail = railItems(chat)
  assert.equal(rail.length, 3, 'the ending adds and drops nothing')
  assert.equal(rail[1].kind, 'reasoning')
  assert.equal(rail[1].text, 'counting')
  assert.equal(rail[2].kind, 'message')
  assert.equal(rail[2].role, 'agent')
  assert.equal(rail[2].text, 'There are ', 'the partial reply is kept, not deleted')
  assert.ok(!rail.some((item) => item.kind === 'stream' || item.kind === 'reasoning-stream'))
})

test('an interrupted turn settles only its own live items', () => {
  let chat = chatFromSession(session({ status: 'working' }))
  foldEvent(chat, event(2, { type: 'message.delta', turn: 1, text: 'first ' }))
  foldEvent(chat, event(3, { type: 'turn.completed', turn: 1, outcome: 'interrupted', coarse: 'stopped' }))
  foldEvent(chat, event(4, { type: 'message.delta', turn: 2, text: 'second ' }))
  assert.equal(railItems(chat).at(-1).kind, 'stream', 'the next turn still streams')
  foldEvent(chat, event(5, { type: 'turn.completed', turn: 2, outcome: 'failed', coarse: 'failed' }))
  const rail = railItems(chat)
  assert.equal(rail[0].kind, 'message')
  assert.equal(rail[1].kind, 'message')
  assert.equal(rail[1].text, 'second ')
})

test('a session that dies mid-reply retires the live reply with no turn event', () => {
  // A harness that exits mid-turn reports the failure as a status. There
  // is no turn.completed to name the turn, and the caret must still go.
  let chat = chatFromSession(session({ status: 'working' }))
  foldEvent(chat, event(2, { type: 'message.delta', turn: 1, text: 'Half a sen' }))
  foldEvent(chat, event(3, { type: 'session.status_changed', status: 'failed', reason: 'the harness exited' }))
  let rail = railItems(chat)
  assert.equal(rail[0].kind, 'message')
  assert.equal(rail[0].text, 'Half a sen')
  assert.equal(chat.lastError, 'the harness exited')

  // A stop ends it the same way.
  const stopped = chatFromSession(session({ status: 'working' }))
  foldEvent(stopped, event(2, { type: 'message.delta', turn: 1, text: 'Halfway' }))
  foldEvent(stopped, event(3, { type: 'session.status_changed', status: 'stopped', reason: null }))
  assert.equal(railItems(stopped)[0].kind, 'message')

  // A turn waiting on the user's answer is not over: its reply is still
  // being written, and its caret is the truth.
  const waiting = chatFromSession(session({ status: 'working' }))
  foldEvent(waiting, event(2, { type: 'message.delta', turn: 1, text: 'I need to ask' }))
  foldEvent(waiting, event(3, { type: 'session.status_changed', status: 'waiting', reason: null }))
  assert.equal(railItems(waiting)[0].kind, 'stream')
})

test('a beat that says nothing new hands back the same record', () => {
  // The poll lands every 2.5 s. A merge that allocates on every beat gives
  // the shell a new chat object each time, and every view keyed on the chat
  // re-renders while a turn is streaming. Identity is the signal that the
  // beat had nothing to say, so it has to survive the merge.
  const payload = session({ status: 'working', title: 'Release triage', updated_at: 1000 })
  const chat = chatFromSession(payload)
  foldEvent(chat, event(1, { type: 'message.delta', turn: 1, text: 'thinking out' }))

  const merged = mergeSessionIntoChat(chat, { ...payload })
  assert.equal(merged, chat, 'an unchanged beat is the same object, not a copy')

  // A payload re-allocated from JSON counts as unchanged too: the arrays a
  // beat carries (approved tools, the queue) are new references every time.
  const withArrays = session({
    approved_tools: ['Bash', 'Edit'],
    queue: [{ id: 'q1', text: 'run the tests' }],
    queue_paused: false,
  })
  const first = mergeSessionIntoChat(chatFromSession(withArrays), withArrays)
  const again = mergeSessionIntoChat(first, {
    ...withArrays,
    approved_tools: ['Bash', 'Edit'],
    queue: [{ id: 'q1', text: 'run the tests' }],
  })
  assert.equal(again, first, 'equal array contents are not a change')
})

test('a beat with a new fact is a new record, and the fold keeps its timeline', () => {
  const chat = chatFromSession(session({ status: 'working' }))
  foldEvent(chat, event(1, { type: 'message.delta', turn: 1, text: 'still writing' }))

  // A fact the merge copies (the model) changed; nothing about the turn did,
  // so the transcript array comes through by reference and the record is new
  // because the beat had something to say.
  const merged = mergeSessionIntoChat(chat, session({ status: 'working', model: 'opus' }))
  assert.notEqual(merged, chat, 'a beat that changed a fact is a new record')
  assert.equal(merged.model, 'opus')
  assert.equal(merged.status, 'running')
  assert.equal(merged.timeline, chat.timeline, 'an unchanged transcript array rides along by reference')

  // A beat that ends the turn is also a new record, and there the merge
  // retires the live block, which is a new array on purpose (pinned above).
  const ended = mergeSessionIntoChat(chat, session({ status: 'idle' }))
  assert.notEqual(ended, chat)
  assert.equal(ended.status, 'idle')
  assert.equal(ended.timeline.at(-1).kind, 'message')
})

test('a beat still wins over a fold when the two disagree', () => {
  // The beat is the fresher snapshot for the identity facts. A stream event
  // that moved the status in between must not turn the next beat into a
  // no-op: the beat's value is the one that wins, which is why the merge
  // compares values rather than trusting the payload to be unchanged.
  const payload = session({ status: 'working', updated_at: 1000 })
  const chat = chatFromSession(payload)
  foldEvent(chat, event(1, { type: 'session.status_changed', status: 'stopped', reason: null }))
  assert.equal(chat.status, 'stopped')

  const merged = mergeSessionIntoChat(chat, { ...payload })
  assert.notEqual(merged, chat)
  assert.equal(merged.status, 'running', 'the beat reasserts what it says')
  assert.equal(merged.running, true)
})

test('a stream gap leaves one note where the history is incomplete', () => {
  const chat = chatFromSession(session())
  const before = chat.timeline.length
  foldEvent(chat, { type: 'stream_gap' })
  assert.equal(chat.timeline.length, before + 1)
  const note = chat.timeline[chat.timeline.length - 1]
  assert.equal(note.role, 'note')
  assert.match(note.text, /could not be read/)
  // A gap carries no seq, so the fold cursor is untouched: a later replay
  // still starts from where the read events left off.
  assert.equal(chat.foldedSeq, null)
  // A second hole (a later run of skips) is its own note, not a repeat of
  // this one.
  foldEvent(chat, { type: 'stream_gap' })
  assert.equal(chat.timeline.filter((i) => i.text === note.text).length, 2)
})

// ---- folding a stream in batches is folding it one event at a time ----
//
// The provider copies a chat and its timeline once per animation frame and
// folds that frame's whole batch against the copy, instead of copying per
// event. That is only safe if the batch is the same fold: one recorded stream
// folded both ways has to produce the same transcript, the same counts and
// the same statuses.

// One turn's worth of every kind the fold knows, in the order a harness sends
// them: a session, reasoning and reply deltas settling into whole items, a
// tool, a decision answered, a note, a gap, an interrupted turn, and a
// duplicate seq a replay must ignore. This is the recorded stream the two
// fold shapes are compared on.
const recordedStream = () => [
  event(1, { type: 'session.created', session: session({ status: 'working' }) }),
  event(2, { type: 'session.status_changed', status: 'working', reason: null }),
  event(3, { type: 'message.added', message: { id: 'm1', turn: 0, role: 'user', text: 'run the migration' } }),
  event(4, { type: 'reasoning.delta', turn: 0, text: 'thinking about ' }),
  event(5, { type: 'reasoning.delta', turn: 0, text: 'the schema' }),
  event(6, { type: 'reasoning.added', turn: 0, text: 'Think about the schema.' }),
  event(7, { type: 'message.delta', turn: 0, text: 'First ' }),
  event(8, { type: 'message.delta', turn: 0, text: 'part.' }),
  event(9, { type: 'tool.started', turn: 0, name: 'Bash', input: { command: 'cargo test' } }),
  event(10, { type: 'tool.result', turn: 0, name: 'Bash', output: 'ok', is_error: false }),
  event(11, { type: 'message.added', message: { id: 'm2', turn: 0, role: 'assistant', text: 'First part.' } }),
  event(12, { type: 'context.used', used: 1200, window: 200000 }),
  event(13, { type: 'usage.cost', cost_usd: 0.02 }),
  event(14, { type: 'thinking.tokens', tokens: 42 }),
  event(15, { type: 'queue.changed', queue: [{ id: 'q1' }], paused: false }),
  event(16, { type: 'session.updated', session: session({ status: 'working', title: 'run the migration' }) }),
  event(17, { type: 'note.added', text: 'a note from the harness' }),
  event(18, {
    type: 'decision.requested',
    decision: {
      id: 'd1',
      state: 'pending',
      harness_request: { tool_name: 'Bash', input: { command: 'rm -rf build' } },
      options: [{ id: 'allow', label: 'Allow' }],
    },
  }),
  event(19, { type: 'decision.responded', decision_id: 'd1', choice: 'allow' }),
  event(20, { type: 'message.delta', turn: 1, text: 'Second turn ' }),
  event(21, { type: 'stream_gap' }),
  event(22, { type: 'daemon.error', message: 'a hiccup' }),
  event(23, { type: 'turn.completed', turn: 1, outcome: 'interrupted' }),
  event(24, { type: 'session.status_changed', status: 'stopped', reason: null }),
  event(25, { type: 'session.status_changed', status: 'failed', reason: 'the harness exited' }),
  // A seq at or below the cursor: a replay, which the fold must ignore.
  event(24, { type: 'message.added', message: { id: 'dup', turn: 1, role: 'assistant', text: 'duplicate' } }),
  event(26, { type: 'turn.started' }),
]

// The shape the provider used before: a fresh chat and timeline per event.
function foldPerEvent(base, stream) {
  let chat = base
  for (const event of stream) chat = foldEvent({ ...chat, timeline: [...chat.timeline] }, event)
  return chat
}

// The shape it uses now: a fresh chat and timeline per frame, and every event
// of that frame folded against the one copy.
function foldPerFrame(base, stream, perFrame) {
  let chat = base
  for (let i = 0; i < stream.length; i += perFrame) {
    chat = { ...chat, timeline: [...chat.timeline] }
    foldEvents(chat, stream.slice(i, i + perFrame))
  }
  return chat
}

test('a stream folded in frames has the transcript of one folded per event', () => {
  const stream = recordedStream()
  const base = chatFromSession(session({ status: 'working' }))
  const once = foldPerEvent(base, stream)

  for (const perFrame of [1, 3, 7, stream.length]) {
    const framed = foldPerFrame(base, stream, perFrame)
    assert.deepEqual(framed, once, `a ${perFrame}-event frame folds to the same chat`)
    // The numbers a person sees, spelled out so a failure says which one moved.
    assert.equal(framed.timeline.length, once.timeline.length, 'same transcript length')
    assert.equal(framed.status, once.status, 'same status')
    assert.equal(framed.running, once.running, 'same running flag')
    assert.equal(framed.pendingDecisionId, once.pendingDecisionId, 'same pending decision')
    assert.equal(framed.lastError, once.lastError, 'same failure sentence')
    assert.equal(framed.costUsd, once.costUsd, 'same running cost')
    assert.equal(framed.thinkingTokens, once.thinkingTokens, 'same thinking total')
    assert.equal(framed.foldedSeq, once.foldedSeq, 'same fold cursor')
    const census = (chat) =>
      chat.timeline.reduce((all, item) => ({ ...all, [item.kind + (item.role ? `:${item.role}` : '')]: (all[item.kind + (item.role ? `:${item.role}` : '')] ?? 0) + 1 }), {})
    assert.deepEqual(census(framed), census(once), 'same items, in the same kinds')
  }
})

test('batching does not double a replayed event or lose a settled one', () => {
  const stream = recordedStream()
  const once = foldPerEvent(chatFromSession(session({ status: 'working' })), stream)
  // The duplicate seq produced no row on either path, and the interrupted
  // turn's live item was retired by both.
  const kinds = once.timeline.map((item) => item.kind)
  assert.equal(kinds.filter((kind) => kind === 'stream').length, 0, 'the interrupted stream was retired')
  assert.equal(once.timeline.some((item) => item.text === 'duplicate'), false, 'the replay added nothing')
  const framed = foldPerFrame(chatFromSession(session({ status: 'working' })), stream, 5)
  assert.equal(framed.timeline.filter((item) => item.kind === 'message').length, once.timeline.filter((item) => item.kind === 'message').length)
})
// ---- pictures riding a chat (br-images 2b770c1) ----

const picture = (blob) => ({ blob, mime: 'image/png' })

test('a user message keeps the pictures that rode it, and a picture alone does not title the chat', () => {
  const chat = foldEvents(chatFromSession(session()), [
    event(1, {
      type: 'message.added',
      message: { id: 'm1', turn: 0, role: 'user', text: '', images: [picture('a'.repeat(64))] },
    }),
    event(2, {
      type: 'message.added',
      message: { id: 'm2', turn: 1, role: 'user', text: 'and this one', images: [picture('b'.repeat(64)), picture('c'.repeat(64))] },
    }),
  ])
  const [first, second] = railItems(chat).filter((item) => item.kind === 'message')
  assert.deepEqual(first.images, [picture('a'.repeat(64))])
  assert.equal(second.images.length, 2)
  assert.equal(chat.title, 'and this one', 'the first words, not the picture, name the chat')
})

test('a message with no pictures carries an empty list, never undefined', () => {
  const chat = foldEvents(chatFromSession(session()), [
    event(1, { type: 'message.added', message: { id: 'm1', turn: 0, role: 'user', text: 'hi' } }),
    event(2, { type: 'message.added', message: { id: 'm2', turn: 0, role: 'assistant', text: 'hello' } }),
  ])
  for (const item of railItems(chat).filter((i) => i.kind === 'message')) assert.deepEqual(item.images, [])
})

test("another agent's message keeps its origin and its pictures together", () => {
  const origin = { kind: 'agent', chat: 'lead', title: 'Release triage', report: true }
  const chat = foldEvents(chatFromSession(session()), [
    event(1, {
      type: 'message.added',
      message: { id: 'm1', turn: 0, role: 'user', text: 'the page now', origin, images: [picture('d'.repeat(64))] },
    }),
  ])
  const message = railItems(chat).find((i) => i.kind === 'message')
  assert.deepEqual(message.origin, origin)
  assert.deepEqual(message.images, [picture('d'.repeat(64))])
})

test("a tool result's pictures land on its card, and a result without any leaves it empty", () => {
  const chat = foldEvents(chatFromSession(session()), [
    event(1, { type: 'tool.started', turn: 0, name: 'mcp__openremote__browser_screenshot', input: {} }),
    event(2, { type: 'tool.started', turn: 0, name: 'Bash', input: { command: 'ls' } }),
    event(3, {
      type: 'tool.result',
      turn: 0,
      name: 'mcp__openremote__browser_screenshot',
      output: 'captured',
      is_error: false,
      images: [picture('e'.repeat(64))],
    }),
    event(4, { type: 'tool.result', turn: 0, name: 'Bash', output: 'a.txt', is_error: false }),
  ])
  const tools = chat.timeline.filter((i) => i.kind === 'tool')
  assert.deepEqual(tools[0].images, [picture('e'.repeat(64))])
  assert.equal(tools[0].result, 'captured')
  assert.deepEqual(tools[1].images, [])
})

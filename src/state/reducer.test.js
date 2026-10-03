import assert from 'node:assert/strict'
import test from 'node:test'
import { chatFromSession, chatStatus, foldEvent, railItems, titleFrom, workspaceName } from './reducer.js'

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

  // The fold itself advances the seq — any path that folds without the
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

  // The settled assistant message replaces the stream — never both.
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

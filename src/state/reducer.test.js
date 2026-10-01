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

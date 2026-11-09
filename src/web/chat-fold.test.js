// A synced range folded into the desktop chat record. The web polls this
// route instead of the daemon's live SSE, so the reducer's own shapes must
// survive the trip: status, queue and the pending decision are what ChatView
// reads to let a person send, steer and answer.

import assert from 'node:assert/strict'
import test from 'node:test'
import { chatFromSession, foldEvents } from '../state/reducer.js'
import { syncedSession } from './sync-view.js'

const row = { id: 'a', executor: 'm1', head: 4, updatedAt: 200 }
const events = [
  {
    seq: 1,
    type: 'session.created',
    session: {
      id: 'a',
      harness: 'claude',
      workspace: 'C:/work/api',
      status: 'working',
      title: null,
      updated_at: 100,
      executor: 'm1',
    },
  },
  {
    seq: 2,
    type: 'message.added',
    message: { id: 'u1', turn: 0, role: 'user', text: 'ship the notes' },
  },
  {
    seq: 3,
    type: 'queue.changed',
    queue: [{ id: 'q1', text: 'then the tests' }],
    paused: false,
  },
  {
    seq: 4,
    type: 'decision.requested',
    decision: {
      id: 'd1',
      session_id: 'a',
      turn: 0,
      kind: 'approval',
      state: 'pending',
      harness_request: { path: 'a.py' },
      options: [{ id: 'allow', label: 'Allow' }],
      created_at: 0,
    },
  },
]

test('a synced range keeps the status, queue and decision ChatView reads', () => {
  const session = syncedSession(row, events)
  const chat = chatFromSession(session)
  const folded = foldEvents(chat, events)
  assert.equal(folded.id, 'a')
  assert.equal(folded.harness, 'claude')
  assert.equal(folded.workspace, 'C:/work/api')
  assert.equal(folded.status, 'running')
  assert.equal(folded.queue.length, 1)
  assert.equal(folded.queue[0].id, 'q1')
  assert.equal(folded.pendingDecisionId, 'd1')
  assert.ok(
    folded.timeline.some((item) => item.kind === 'decision' && item.id === 'd1'),
    'the decision is in the transcript',
  )
  assert.ok(
    folded.timeline.some((item) => item.role === 'user' && item.text === 'ship the notes'),
    'the prompt is in the transcript',
  )
})

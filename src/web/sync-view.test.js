// The rules of the web device view: what the synced-chat summary may say, how
// the poll cursor moves, what a refused chat means, and the shape of a prompt
// request. Pure, because a browser view that only works when a machine is
// awake is a view nobody can test.

import assert from 'node:assert/strict'
import test from 'node:test'
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
  hydrateTitles,
  pageSize,
  promptRequest,
  relativeWhen,
  rowLine,
  rowTitle,
  mergeSyncedSummaries,
  shortId,
  summaryList,
  syncedSession,
  summaryRow,
} from './sync-view.js'

const page = (...seqs) => ({ events: seqs.map((seq) => ({ seq, type: 'message.added' })), head: Math.max(0, ...seqs) })

test('the summary lists the synced chats newest first and never lists a tombstoned one', () => {
  const rows = summaryList(
    {
      v: 1,
      sessions: [
        { id: 'old', executor: 'build-box', head: 10, updated_at: 1000 },
        { id: 'deleted', executor: 'build-box', head: 4, updated_at: 9000 },
        { id: 'new', executor: 'studio-mini', head: 2, updated_at: 5000 },
        null,
        7,
        { id: '' },
      ],
      tombstones: [{ id: 'deleted', at: 9000, by: 'build-box' }, {}, null],
    },
    {},
  )
  assert.deepEqual(rows.map((row) => row.id), ['new', 'old'])
  assert.equal(rows[0].executor, 'studio-mini')
  assert.equal(rows[0].head, 2)
})

test('a summary row carries only what the summary route sends', () => {
  const row = summaryRow({ id: 'a1', executor: 'build-box', head: 3, updated_at: 7, title: 'not in the summary', harness: 'claude' })
  assert.deepEqual(Object.keys(row).sort(), ['executor', 'head', 'id', 'title', 'updatedAt'].sort())
  // A title is never invented from a field the route does not send; it comes
  // from the log once the chat is opened.
  assert.equal(row.title, null)
  assert.equal(summaryRow({ id: 'a1', executor: 'build-box', head: 3, updated_at: 7 }, { a1: 'Named in the log' }).title, 'Named in the log')
  // The executor is a device id, and this page is already showing that
  // machine, so the row names the place rather than repeating the id.
  assert.equal(rowLine({ id: 'a1', executor: 'build-box', updatedAt: 0 }), '')
  // A titled row carries only its time; an untitled one keeps the short id
  // so two unreadable rows are still distinguishable.
  assert.equal(rowLine({ id: 'a1', updatedAt: 0 }, { when: () => '2:32 PM' }), '2:32 PM')
  assert.equal(
    rowLine({ id: 'a1', updatedAt: 0 }, { id: 'a1', when: () => '2:32 PM' }),
    'a1 · last changed 2:32 PM',
  )
})

test('the empty list says what it is and how a chat gets synced', () => {
  assert.match(EMPTY_SUMMARY, /No chats are synced from this computer yet/)
  assert.match(EMPTY_SUMMARY, /desktop app/)
  assert.match(EMPTY_SUMMARY, /Sync/)
})

test('the cursor follows the events actually read, never the head', () => {
  assert.equal(advanceCursor(0, page(1, 2, 3)), 3)
  // The same page again moves nothing: a repeated request cannot advance past
  // what was folded.
  assert.equal(advanceCursor(3, page(1, 2, 3)), 3)
  // A gap still advances the cursor past it, or the next poll asks for the same
  // gap for ever.
  assert.equal(advanceCursor(3, { events: [{ seq: 7 }, { seq: 8 }], head: 8 }), 8)
  // A head beyond the page is more to fetch, not a position to jump to: the
  // range route stops at a byte budget.
  assert.equal(advanceCursor(8, { events: [{ seq: 9 }], head: 40 }), 9)
  assert.equal(advanceCursor(9, { events: [], head: 40 }), 9)
  assert.equal(advanceCursor(0, { events: [{ seq: 'x' }, null], head: 2 }), 0)
})

test('a page counts what it carried so an empty answer is not a catch-up', () => {
  assert.equal(pageSize(page(1)), 1)
  assert.equal(pageSize({ events: [] }), 0)
  assert.equal(pageSize(null), 0)
})

test('410 is deletion and stops the poll; 404 is private or gone and also stops', () => {
  const deleted = chatRefusal({ status: 410 })
  assert.match(deleted.sentence, /deleted/)
  assert.equal(deleted.stop, true)
  const missing = chatRefusal({ status: 404 })
  assert.equal(missing.stop, true)
  const failed = chatRefusal(new Error('offline'))
  assert.equal(failed.stop, false)
  assert.match(failed.sentence, /keeps trying/)
})

test('a prompt is a POST carrying a request_id and the text', () => {
  const request = promptRequest({ id: 'abc', text: 'hello', requestId: 'req-1' })
  assert.equal(request.method, 'POST')
  assert.equal(request.path, '/sessions/abc/prompts')
  assert.deepEqual(request.body, { request_id: 'req-1', text: 'hello' })
  assert.equal(canSend('hi'), true)
  assert.equal(canSend('   '), false)
  assert.equal(canSend(null), false)
})

test('the answer body is the desktop console sent: request_id and choice, no others', () => {
  const request = answerRequest({ decisionId: 'd1', choice: 'allow', requestId: 'req-9' })
  assert.equal(request.method, 'POST')
  assert.equal(request.path, '/decisions/d1/answer')
  assert.deepEqual(Object.keys(request.body).sort(), ['choice', 'request_id'])
  assert.deepEqual(request.body, { request_id: 'req-9', choice: 'allow' })
})

test('a decision cannot be answered twice, and an answered row names the option label', () => {
  assert.equal(canAnswerDecision({ pending: true, choice: 'allow' }), true)
  assert.equal(canAnswerDecision({ pending: true, answering: true, choice: 'allow' }), false, 'one request at a time')
  assert.equal(canAnswerDecision({ pending: true, answeredChoice: 'allow', choice: 'allow' }), false, 'already answered')
  assert.equal(canAnswerDecision({ pending: false, choice: 'allow' }), false, 'a settled decision is not answerable')
  assert.equal(canAnswerDecision({ pending: true }), false, 'no choice to send')
  const item = { pending: false, answeredChoice: 'allow', options: [{ id: 'allow', label: 'Allow this command' }, { id: 'deny', label: 'Deny' }] }
  assert.equal(answerLabel(item), 'Allow this command', 'the label, never the wire id')
  assert.deepEqual(decisionOptions({ options: [{ id: 'allow', label: 'Allow' }, null, { label: 'no id' }] }).map((o) => o.id), ['allow'])
})

test('a refusal shows the daemon sentence and leaves the row answerable', () => {
  assert.equal(answerRefusal({ message: 'no such decision' }), 'no such decision')
  assert.equal(answerRefusal(new Error('')), 'The computer refused the answer.')
  // The row was sending, the request came back refused: the answering flag is
  // cleared and the same row can be answered again.
  assert.equal(canAnswerDecision({ pending: true, answering: false, choice: 'deny' }), true)
})

// ---- the list as a product surface, not a sync inspector ----

const now = new Date(2026, 10, 9, 12, 0, 0).getTime()
const day = (offset) => {
  const date = new Date(now)
  date.setDate(date.getDate() + offset)
  return date.getTime()
}

test('the list groups chats by the day they last changed', () => {
  const rows = [
    { id: 'today', updatedAt: day(0) },
    { id: 'yesterday', updatedAt: day(-1) },
    { id: 'week', updatedAt: day(-3) },
    { id: 'old', updatedAt: day(-30) },
  ]
  assert.deepEqual(
    groupByDay(rows, now).map((group) => [group.label, group.rows.map((row) => row.id)]),
    [
      ['Today', ['today']],
      ['Yesterday', ['yesterday']],
      ['Previous 7 days', ['week']],
      ['Earlier', ['old']],
    ],
  )
  assert.deepEqual(groupByDay([], now), [])
})

test('a row is named by its real title, never by its id', () => {
  assert.equal(rowTitle({ id: 'f36e8ec59', title: 'Ship the notes' }), 'Ship the notes')
  assert.equal(rowTitle({ id: 'f36e8ec59' }, { f36e8ec59: 'Named later' }), 'Named later')
  assert.equal(rowTitle({ id: 'f36e8ec59' }), 'Untitled chat')
  assert.equal(shortId('f36e8ec59-aaaa'), 'f36e8ec5')
})

test('the row time is a clock today and a date before that', () => {
  assert.match(relativeWhen(day(0), now), /\d/)
  assert.match(relativeWhen(day(-3), now), /[A-Za-z]/)
  assert.equal(relativeWhen(0, now), '')
  assert.equal(relativeWhen(NaN, now), '')
})

test('title hydration is capped and never reads more than three at once', async () => {
  let active = 0
  let peak = 0
  let calls = 0
  const rows = Array.from({ length: 20 }, (_, i) => ({ id: `r${i}` }))
  const read = async (row) => {
    calls += 1
    active += 1
    peak = Math.max(peak, active)
    await new Promise((resolve) => setTimeout(resolve, 1))
    active -= 1
    return `Title ${row.id}`
  }
  const titles = {}
  await hydrateTitles(rows, read, {
    onTitle: (id, title) => {
      titles[id] = title
    },
  })
  assert.equal(calls, 12)
  assert.ok(peak <= 3, `peak concurrency ${peak}`)
  assert.equal(Object.keys(titles).length, 12)
})

test('a cancelled hydration stops scheduling reads', async () => {
  let cancelled = false
  let calls = 0
  const read = async () => {
    calls += 1
    cancelled = true
    return 'x'
  }
  await hydrateTitles([{ id: 'a' }, { id: 'b' }, { id: 'c' }, { id: 'd' }], read, {
    isCancelled: () => cancelled,
  })
  assert.ok(calls <= 3, `started ${calls} reads after cancellation`)
})
// ---- the web desktop: synced rows become the desktop Session ----

test('a synced row becomes the Session its own log carries', () => {
  const events = [
    {
      seq: 1,
      payload: {
        type: 'session.created',
        session: {
          id: 'a',
          harness: 'claude',
          workspace: 'C:/work/api',
          status: 'idle',
          model: 'opus',
          effort: 'high',
          permission_mode: 'plan',
          fast: true,
          title: null,
          updated_at: 100,
          executor: 'm1',
        },
      },
    },
    {
      seq: 2,
      payload: {
        type: 'session.updated',
        session: {
          id: 'a',
          harness: 'claude',
          workspace: 'C:/work/api',
          status: 'working',
          model: 'opus',
          effort: 'high',
          permission_mode: 'plan',
          fast: true,
          title: 'Ship the notes',
          updated_at: 200,
          executor: 'm1',
        },
      },
    },
  ]
  const session = syncedSession({ id: 'a', executor: 'm1', head: 2, updatedAt: 150 }, events)
  assert.equal(session.harness, 'claude')
  assert.equal(session.workspace, 'C:/work/api')
  assert.equal(session.title, 'Ship the notes')
  assert.equal(session.status, 'working')
  assert.equal(session.model, 'opus')
  assert.equal(session.head, 2)
  assert.equal(session.updated_at, 200, 'the log is newer than the summary')
})

test('a row with no session event still opens as an idle chat', () => {
  const session = syncedSession({ id: 'a', executor: 'm1', head: 0, updatedAt: 0 }, [])
  assert.equal(session.id, 'a')
  assert.equal(session.executor, 'm1')
  assert.equal(session.status, 'idle')
  assert.equal(session.harness, null)
  assert.equal(session.title, null)
})

test('machine summaries merge by chat id and drop a tombstone', () => {
  const rows = mergeSyncedSummaries([
    {
      machine: 'm1',
      payload: {
        sessions: [
          { id: 'a', executor: 'm1', head: 3, updated_at: 10 },
          { id: 'b', executor: 'm1', head: 1, updated_at: 5 },
        ],
        tombstones: [],
      },
    },
    {
      machine: 'm2',
      payload: {
        sessions: [
          { id: 'a', executor: 'm1', head: 4, updated_at: 20 },
          { id: 'c', executor: 'm2', head: 2, updated_at: 30 },
        ],
        tombstones: [{ id: 'b' }],
      },
    },
  ])
  assert.deepEqual(rows.map((row) => row.id), ['c', 'a'])
  assert.equal(rows.find((row) => row.id === 'a').head, 4)
  assert.equal(rows.find((row) => row.id === 'a').updatedAt, 20)
})

// Stopping one harness's chats: who is offered, in what order, what the page
// and the confirm say, and what the outcome sentence names. Every rule here
// is either a number a person reads before pressing or a sentence they read
// after.

import assert from 'node:assert/strict'
import test from 'node:test'
import { stopSummary } from './stop-everything.js'
import {
  HARNESS_PICK_LINE,
  HARNESS_STOP_NAME,
  harnessQuestion,
  harnessStopSub,
  harnessStopping,
  harnessStops,
  harnessSummary,
} from './stop-harness.js'

// A session as the daemon lists it: the harness that runs it, its status,
// and the device a synced chat runs on (null for a private chat here).
const session = (over = {}) => ({
  id: 's1',
  harness: 'claude',
  status: 'working',
  device_id: null,
  title: 'A task',
  created_at: 1,
  ...over,
})

test('only harnesses with a chat this computer can stop are offered', () => {
  const rows = harnessStops(
    [
      session({ id: 'a', harness: 'claude' }),
      // All of this one runs on another computer: nothing to stop here.
      session({ id: 'b', harness: 'codex', device_id: 'box' }),
      // Not running: not part of any stop.
      session({ id: 'c', harness: 'grok', status: 'idle' }),
      // A sync that the daemon holds for another computer only.
      session({ id: 'd', harness: 'pi', status: 'waiting', device_id: 'box' }),
    ],
    { thisDevice: 'me' },
  )
  assert.deepEqual(rows.map((r) => r.id), ['claude'])
  assert.deepEqual(rows[0], { id: 'claude', name: 'Claude Code', count: 1, elsewhere: 0 })
})

test('a harness keeps its chats that run elsewhere, counted apart', () => {
  const rows = harnessStops(
    [
      session({ id: 'a', harness: 'claude' }),
      session({ id: 'b', harness: 'claude', device_id: 'box' }),
      session({ id: 'c', harness: 'claude', device_id: 'me' }),
      session({ id: 'd', harness: 'claude', device_id: 'box' }),
    ],
    { thisDevice: 'me' },
  )
  assert.equal(rows.length, 1)
  assert.equal(rows[0].count, 2, 'the two this computer holds')
  assert.equal(rows[0].elsewhere, 2, 'and the two it does not')
})

test('the order is the count first, then the harness name', () => {
  const rows = harnessStops([
    session({ id: 'a', harness: 'grok' }),
    session({ id: 'b', harness: 'claude' }),
    session({ id: 'c', harness: 'claude' }),
    session({ id: 'd', harness: 'codex' }),
    session({ id: 'e', harness: 'codex' }),
    session({ id: 'f', harness: 'codex' }),
  ])
  assert.deepEqual(rows.map((r) => r.id), ['codex', 'claude', 'grok'])
  // Ties keep a stable order: the console's name for the harness.
  const tie = harnessStops([session({ id: 'a', harness: 'pi' }), session({ id: 'b', harness: 'agy' })])
  assert.deepEqual(tie.map((r) => r.id), ['agy', 'pi'], 'Antigravity before Pi Agent')
})

test('the empty case offers nothing at all', () => {
  assert.deepEqual(harnessStops([]), [])
  assert.deepEqual(harnessStops(null), [])
  // Running chats, all elsewhere: still nothing this page could stop.
  assert.deepEqual(harnessStops([session({ device_id: 'box' })], { thisDevice: 'me' }), [])
  // Not running, whatever the harness.
  assert.deepEqual(harnessStops([session({ status: 'idle' }), session({ status: 'stopped' })]), [])
})

test('the root row says how much work the page would end', () => {
  assert.equal(harnessStopSub([{ id: 'claude', name: 'Claude Code', count: 2 }]), '2 chats in Claude Code')
  assert.equal(harnessStopSub([{ id: 'claude', name: 'Claude Code', count: 1 }]), '1 chat in Claude Code')
  assert.equal(
    harnessStopSub([
      { id: 'codex', name: 'Codex', count: 3 },
      { id: 'claude', name: 'Claude Code', count: 2 },
    ]),
    '5 chats in 2 apps',
  )
  assert.equal(harnessStopSub([]), '')
})

test('the page and the confirm say the scope and the count', () => {
  assert.equal(HARNESS_STOP_NAME, "Stop one app's chats")
  assert.equal(
    HARNESS_PICK_LINE,
    'Only chats running on this computer can stop; the rest keep running.',
  )
  assert.equal(harnessQuestion('Claude Code', 1), 'Stop 1 chat in Claude Code?')
  assert.equal(harnessQuestion('Claude Code', 3), 'Stop 3 chats in Claude Code?')
  assert.equal(harnessStopping('Claude Code', 1), 'Stopping 1 chat in Claude Code.')
  assert.equal(harnessStopping('Claude Code', 3), 'Stopping 3 chats in Claude Code.')
})

test('the outcome names the harness and keeps stop-everything sentences verbatim', () => {
  assert.equal(harnessSummary({ name: 'Claude Code', stopped: 2 }), 'Stopped 2 chats in Claude Code.')
  assert.equal(harnessSummary({ name: 'Claude Code', stopped: 1 }), 'Stopped 1 chat in Claude Code.')
  // The skipped, gone and failed sentences are the stop-all ones, unchanged.
  assert.equal(
    harnessSummary({ name: 'Claude Code', stopped: 2, elsewhere: 1 }),
    'Stopped 2 chats in Claude Code. One is on another computer, so it kept running.',
  )
  assert.equal(
    harnessSummary({ name: 'Claude Code', stopped: 1, gone: 2, failed: 1 }),
    'Stopped 1 chat in Claude Code. Two were already gone. One did not answer.',
  )
  // Nothing stoppable, only chats elsewhere: said without pretending.
  assert.equal(
    harnessSummary({ name: 'Claude Code', stopped: 0, elsewhere: 2 }),
    'Nothing was stopped in Claude Code. Two are on other computers, so they kept running.',
  )
  assert.equal(harnessSummary({ name: 'Claude Code' }), 'Nothing was stopped in Claude Code.')
})

test('every tail sentence is the one stop-everything composes for the same counts', () => {
  // The point of reusing rather than forking: my head plus their words is the
  // sentence; nothing here paraphrases their copy.
  for (const counts of [
    { stopped: 3, elsewhere: 1, gone: 1, failed: 1 },
    { stopped: 0, elsewhere: 3 },
    { stopped: 2, gone: 2 },
    { stopped: 0 },
  ]) {
    const theirs = stopSummary(counts)
    const mine = harnessSummary({ name: 'Claude Code', ...counts })
    const tail = theirs.replace(/^[^.]*\.\s*/, '')
    assert.equal(mine.endsWith(tail), true, `tail of ${JSON.stringify(counts)} is theirs`)
  }
})

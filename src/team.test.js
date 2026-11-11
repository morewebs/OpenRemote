// Agent teams as the console derives them from the session list: who works
// for whom, and what a lead's status says of its whole team.

import assert from 'node:assert/strict'
import test from 'node:test'
import { descendantsOf, isTeamTool, leadOf, rolledStatus, teamDot, workersOf } from './team.js'

const chat = (id, status, parent = null, at = 0) => ({
  id,
  status,
  parent: parent ? { chat: parent, depth: 1, spawned_at: at } : undefined,
})

const sessions = [
  chat('lead', 'idle'),
  chat('b', 'working', 'lead', 20),
  chat('a', 'idle', 'lead', 10),
  chat('a1', 'waiting', 'a', 30),
  chat('other', 'failed'),
]

test('workers come in the order they were started', () => {
  assert.deepEqual(workersOf(sessions, 'lead').map((s) => s.id), ['a', 'b'])
})

test('everything under a chat, parents before their own workers, with depth', () => {
  assert.deepEqual(
    descendantsOf(sessions, 'lead').map(({ session, depth }) => [session.id, depth]),
    [['a', 1], ['a1', 2], ['b', 1]],
  )
})

test('a loop in the links ends where it closes', () => {
  const loop = [chat('x', 'idle', 'y'), chat('y', 'idle', 'x')]
  assert.deepEqual(descendantsOf(loop, 'x').map(({ session }) => session.id), ['y'])
})

test('a worker knows its lead, and a chat with none has none', () => {
  assert.equal(leadOf(sessions, sessions[1]).id, 'lead')
  assert.equal(leadOf(sessions, sessions[0]), null)
})

test('waiting beats failed beats running, and a quiet team leaves the own status', () => {
  assert.equal(rolledStatus('idle', ['running', 'waiting']), 'waiting')
  assert.equal(rolledStatus('running', ['failed']), 'failed')
  assert.equal(rolledStatus('idle', ['running']), 'running')
  assert.equal(rolledStatus('idle', ['idle', 'stopped']), 'idle')
})

test('the team dot speaks for what is under a chat alone', () => {
  assert.equal(teamDot(sessions, 'lead'), 'waiting')
  assert.equal(teamDot(sessions, 'other'), null)
})

test('only the team tools are team tools', () => {
  assert.ok(isTeamTool('mcp__openremote__agents_spawn'))
  assert.ok(!isTeamTool('mcp__openremote__browser_click'))
  assert.ok(!isTeamTool(undefined))
})

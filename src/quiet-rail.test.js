import assert from 'node:assert/strict'
import test from 'node:test'
import { quietRail, toolHint, workLabel } from './quiet-rail.js'

test('thinking and tools collapse into one row between the messages', () => {
  const rail = quietRail([
    { kind: 'message', id: 'u', role: 'user', text: 'who are you' },
    { kind: 'reasoning', id: 'r', text: 'long thought' },
    { kind: 'tool', id: 't1', name: 'skill', input: { id: 'opencode' }, result: 'huge' },
    { kind: 'tool', id: 't2', name: 'webfetch', input: { url: 'https://opencode.ai/v2/llms.txt' }, result: 'page' },
    { kind: 'message', id: 'a', role: 'assistant', text: 'I am Grok.' },
  ])
  assert.equal(rail.length, 3)
  assert.equal(rail[1].kind, 'work')
  assert.equal(rail[1].items.length, 3)
  assert.equal(workLabel(rail[1].items, false), 'skill, webfetch')
  assert.equal(toolHint(rail[1].items[2]), 'opencode.ai/v2/llms.txt')
})

test('a question that is still waiting stays in the thread', () => {
  const rail = quietRail([
    { kind: 'tool', id: 't', name: 'bash', result: null },
    { kind: 'decision', id: 'd', pending: true },
  ])
  assert.equal(rail[0].kind, 'work')
  assert.equal(rail[1].kind, 'decision')
})

test('a live step names the tool that is running', () => {
  assert.equal(
    workLabel([{ kind: 'tool', name: 'webfetch', result: null }], true),
    'webfetch',
  )
  assert.equal(workLabel([{ kind: 'reasoning-stream' }], true), 'Thinking…')
})

test('a browser step names what it acted on in the opened work row', async () => {
  const { toolLabel } = await import('./quiet-rail.js')
  const click = { kind: 'tool', id: 'b1', name: 'mcp__openremote__browser_click', input: { element: 'Sign in', url: 'https://a.test/' } }
  const nav = { kind: 'tool', id: 'b2', name: 'mcp__openremote__browser_navigate', input: { url: 'https://docs.rs/serde' } }
  const press = { kind: 'tool', id: 'b3', name: 'mcp__openremote__browser_press', input: { key: 'Enter' } }
  assert.equal(toolLabel(click), 'Browser · click')
  assert.equal(toolLabel({ name: 'bash' }), 'bash')
  assert.equal(toolHint(click), 'Sign in')
  assert.equal(toolHint(nav), 'docs.rs/serde')
  assert.equal(toolHint(press), 'Enter')
})

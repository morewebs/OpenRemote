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

test('an opened step: a browser step in words with its output, a page read folded', async () => {
  const { workStep } = await import('./quiet-rail.js')
  const click = workStep({ kind: 'tool', id: 'b1', name: 'mcp__openremote__browser_click', input: { element: 'Sign in', url: 'https://a.test/' }, result: 'clicked' })
  assert.equal(click.label, 'Browser · click')
  assert.equal(click.hint, null, 'the facts replace the hint')
  assert.equal(click.browser.description, 'Sign in')
  assert.equal(click.out, 'clicked')
  assert.equal(click.fold, false)
  const read = workStep({ kind: 'tool', id: 'b2', name: 'mcp__openremote__browser_snapshot', input: {}, result: 'a long page' })
  assert.equal(read.label, 'Browser · read page')
  assert.equal(read.fold, true, 'a page read as text folds')
  const team = workStep({ kind: 'tool', id: 't', name: 'agents_start', input: { brief: 'x' }, result: '{"chat":"c1"}' })
  assert.equal(team.browser, null)
  assert.equal(team.out, '{"chat":"c1"}', 'a team tool answers in the same Out row')
  assert.equal(team.fold, false)
  const plain = workStep({ kind: 'tool', id: 'w', name: 'webfetch', input: { url: 'https://docs.rs/serde' }, result: null })
  assert.equal(plain.hint, 'docs.rs/serde')
  assert.equal(plain.out, null)
})

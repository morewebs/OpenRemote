import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { quietRail, railActivity, reasoningLabel, toolHint, workLabel, workStep } from './quiet-rail.js'

const prompt = { kind: 'message', id: 'prompt', role: 'user' }
const activeStep = { kind: 'tool', id: 'step', name: 'Read', result: null }
const currentActivity = (status, items, options) => railActivity(quietRail(items), status, options)

test('quiet active turns distinguish App startup from waiting on its response', () => {
  assert.deepEqual(currentActivity('starting', []), { label: 'Starting the App', waitKey: 'starting' })
  const first = currentActivity('working', [prompt])
  assert.deepEqual(first, { label: 'Untangling', waitKey: 'reply-prompt' })
  assert.deepEqual(currentActivity('running', [prompt, { ...activeStep, result: 'done' }]), first)
  assert.deepEqual(currentActivity('working', [prompt, { ...prompt, id: 'prompt-2' }]), {
    label: 'Following the thread', waitKey: 'reply-prompt-2',
  })
})

test('thinking keeps a quiet row and a separate wait line until the next step', () => {
  assert.deepEqual(currentActivity('running', [activeStep, prompt]), currentActivity('running', [prompt]))
  const thinking = { kind: 'reasoning-stream', id: 'thinking' }
  const preamble = { kind: 'message', id: 'preamble', role: 'agent' }
  assert.deepEqual(currentActivity('running', [prompt, thinking]), {
    ...currentActivity('running', [prompt]), reasoningId: 'thinking',
  })
  assert.equal(currentActivity('running', [prompt, thinking, { ...activeStep, result: 'done' }])?.reasoningId, undefined)
  assert.deepEqual(currentActivity('running', [prompt, thinking, preamble, activeStep]), { workId: 'work-step' })
})

test('a reply, a decision, or an inactive or unreachable chat needs no wait line', () => {
  const stream = { kind: 'stream', id: 'reply' }
  assert.equal(currentActivity('working', [prompt, stream, activeStep]), null)
  const settled = { kind: 'message', id: 'settled', role: 'agent' }
  assert.equal(currentActivity('working', [prompt, activeStep, settled]), null)
  assert.ok(currentActivity('working', [settled, prompt])?.label, 'the next prompt gets a wait line')
  assert.equal(currentActivity('working', [prompt, activeStep, { kind: 'decision', pending: true }]), null)
  for (const status of ['waiting', 'idle', 'stopped', 'failed']) {
    assert.equal(currentActivity(status, [prompt, activeStep]), null, status)
  }
  assert.equal(currentActivity('working', [prompt], { reachable: false }), null)
})

test('the folding ellipsis opts in only when both motion preferences allow it', () => {
  const css = readFileSync(new URL('./chatview.css', import.meta.url), 'utf8')
  assert.match(css, /@media \(prefers-reduced-motion: no-preference\) and \(forced-colors: none\) \{\s*html:not\(\[data-reduce-motion\]\) \.cv-activity-fold > i\s*\{[^}]*animation: cv-activity-fold/)
})

test('thinking has its own row while consecutive tools stay grouped', () => {
  const rail = quietRail([
    { kind: 'message', id: 'u', role: 'user', text: 'who are you' },
    { kind: 'reasoning', id: 'r', text: 'long thought' },
    { kind: 'tool', id: 't1', name: 'skill', input: { id: 'opencode' }, result: 'huge' },
    { kind: 'tool', id: 't2', name: 'webfetch', input: { url: 'https://opencode.ai/v2/llms.txt' }, result: 'page' },
    { kind: 'message', id: 'a', role: 'assistant', text: 'I am Grok.' },
  ])
  assert.equal(rail.length, 4)
  assert.equal(rail[1].kind, 'reasoning')
  assert.equal(rail[2].kind, 'work')
  assert.equal(rail[2].items.length, 2)
  assert.equal(workLabel(rail[2].items, false), 'skill, webfetch')
  assert.equal(toolHint(rail[2].items[1]), 'opencode.ai/v2/llms.txt')
})

test('thinking copy only shows tokens and elapsed time when the App reported enough to know', () => {
  assert.equal(reasoningLabel({ live: true, tokens: 33 }), 'Thinking... \u00b7 33 tokens')
  assert.equal(reasoningLabel({ live: true, tokens: 1 }), 'Thinking... \u00b7 1 token')
  assert.equal(reasoningLabel({ live: true }), 'Thinking...')
  assert.equal(reasoningLabel({ durationMs: 3100 }), 'Thought for 3s')
  assert.equal(reasoningLabel({ durationMs: 65900 }), 'Thought for 1m 5s')
  assert.equal(reasoningLabel({ durationMs: 555 }), 'Thought for 0 seconds')
  assert.equal(reasoningLabel({ durationMs: null, tokens: 5 }), 'Thought')
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

const run = (id, command, result = 'out') => ({ kind: 'tool', id, name: 'commandExecution', input: { command }, result })

test('a Codex command reads as the command it ran, never as the protocol item', () => {
  assert.equal(workLabel([run('a', 'ls')], false), 'Ran a command')
  assert.equal(workLabel([run('a', 'ls'), run('b', 'pwd'), run('c', 'id')], false), 'Ran 3 commands')
  // Counted with the other tools of the row, not in place of them.
  const mixed = [run('a', 'ls'), run('b', 'pwd'), { kind: 'tool', id: 'w', name: 'WebSearch', input: { query: 'x' }, result: 'r' }]
  assert.equal(workLabel(mixed, false), 'Ran 2 commands, WebSearch')
  for (const label of [workLabel(mixed, false), workStep(mixed[0]).label]) assert.ok(!label.includes('commandExecution'))
})

test('a running command names what it is running, without its shell wrapper', () => {
  const running = { kind: 'tool', id: 'a', name: 'commandExecution', input: { command: "powershell.exe -Command 'rg -n x src'" }, result: null }
  assert.equal(workLabel([running], true), 'Running rg -n x src')
  assert.equal(workLabel([{ ...running, input: {} }], true), 'Running a command')
  // Another harness's own tool name is its own word and stays.
  assert.equal(workLabel([{ kind: 'tool', id: 'b', name: 'Bash', input: { command: 'ls' }, result: null }], true), 'Bash')
})

test('an opened command step shows the command that ran', () => {
  const step = workStep(run('a', '/bin/bash -lc "git status --short"'))
  assert.equal(step.label, 'Ran')
  assert.equal(step.hint, 'git status --short')
  assert.equal(toolHint(run('a', 'npm test')), 'npm test')
})

test('garbled characters in a command\'s output are repaired, and the received text is kept', () => {
  const garbled = new TextDecoder('windows-1252').decode(Buffer.from('it\u2019s done \u2705', 'utf8'))
  const step = workStep(run('a', 'Get-Content notes.md', garbled))
  assert.equal(step.out, 'it\u2019s done \u2705')
  assert.equal(step.repaired, true)
  assert.equal(step.received, garbled)
  // Clean output is untouched, and a tool that is not a command is not read.
  assert.equal(workStep(run('a', 'ls', 'plain')).repaired, false)
  assert.equal(workStep({ kind: 'tool', id: 'f', name: 'Read', input: { file_path: 'a.md' }, result: garbled }).out, garbled)
})

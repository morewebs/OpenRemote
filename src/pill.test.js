import assert from 'node:assert/strict'
import test from 'node:test'
import { parsePill } from './pill.js'

const ctx = {
  machines: [
    { id: 'm-this', name: 'morewebdev', status: 'online' },
    { id: 'm-box', name: 'build-box', status: 'waiting' },
  ],
  projects: ['webapp', 'api', 'site'],
  defaults: { harness: 'claude', machineId: 'm-this' },
}

test('a sentence without a when is not a rule — guidance, not a draft', () => {
  const out = parsePill('what can you do?', ctx)
  assert.equal(out.reply != null, true)
  assert.equal(out.draft, undefined)
  assert.match(out.reply, /Describe when it should fire/)
})

test('a rule-shaped sentence drafts with its own words', () => {
  const out = parsePill(
    'When the api pipeline on main fails, open a chat with codex to bisect the flaky test.',
    ctx,
  )
  assert.equal(out.draft.kind, undefined)
  const d = out.draft
  assert.equal(d.trigger.kind, 'pipeline')
  assert.equal(d.trigger.project, 'api')
  assert.equal(d.trigger.branch, 'main')
  assert.equal(d.harness, 'codex')
  assert.equal(d.task, 'bisect the flaky test')
  assert.equal(d.machineId, 'm-this')
})

test('schedule words and times shape the draft', () => {
  const out = parsePill('make a nightly dependency audit at 23:30 with claude', ctx)
  const d = out.draft
  assert.equal(d.trigger.kind, 'schedule')
  assert.equal(d.trigger.time, '23:30')
  assert.equal(d.harness, 'claude')
})

test('a waiting machine named in the sentence does not ride the draft', () => {
  const out = parsePill('when a webhook arrives, open a chat on build-box to deploy', ctx)
  const d = out.draft
  assert.equal(d.trigger.kind, 'webhook')
  assert.equal(d.machineId, 'm-this')
  assert.equal(d.task, 'deploy')
})

test('an online machine rides the draft when it is mentioned', () => {
  const out = parsePill('when a webhook arrives, open a chat on morewebdev to deploy', ctx)
  const d = out.draft
  assert.equal(d.machineId, 'm-this')
  assert.equal(d.task, 'deploy')
})

test('a "then" sentence carries its tail as the task', () => {
  const out = parsePill('create a rule that reviews the api every day at 9:00 then read the diff', ctx)
  const d = out.draft
  assert.equal(d.trigger.kind, 'schedule')
  assert.equal(d.task, 'read the diff')
})

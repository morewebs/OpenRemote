import test from 'node:test'
import assert from 'node:assert/strict'
import { appSelection, carryAppDraft } from './app-selection.js'

const chat = { id: 'chat-a', harness: 'claude', model: 'opus', effort: 'high', permissionMode: 'acceptEdits', workspace: '/work/app' }
const codex = { id: 'codex', available: true, enabled: true }

test('choosing another App preserves the project without copying another Apps model or permissions', () => {
  const result = appSelection(chat, codex, { thisDevice: 'here', projects: [{ id: 'project-a', folders: ['/work/app'] }] })
  assert.deepEqual(result, { harness: 'codex', target: null, workspace: '/work/app', projectId: 'project-a' })
  assert.equal(chat.harness, 'claude')
})

test('remote selection stays on the chats machine and carries its folder rather than a local project', () => {
  const result = appSelection({ ...chat, executor: 'remote' }, codex, { thisDevice: 'here', projects: [{ id: 'wrong-local-project', folders: ['/work/app'] }] })
  assert.deepEqual(result, { harness: 'codex', target: 'remote', workspace: '/work/app' })
})

test('the current App and unavailable or disabled Apps do not start a new composer', () => {
  for (const app of [{ ...codex, id: 'claude' }, { ...codex, available: false }, { ...codex, enabled: false }, null]) {
    assert.equal(appSelection(chat, app), null)
  }
})

test('carrying a draft keeps an existing unsent New chat draft and avoids duplicating the same text', () => {
  assert.equal(carryAppDraft('already waiting', 'next task'), 'already waiting\n\nnext task')
  assert.equal(carryAppDraft('next task', 'next task'), 'next task')
  assert.equal(carryAppDraft('', 'next task'), 'next task')
  assert.equal(carryAppDraft('already waiting', ''), 'already waiting')
})

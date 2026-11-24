import assert from 'node:assert/strict'
import test from 'node:test'
import { chatSendActions } from './chat-send.js'

test('the send spot stops a live reply but makes room for a draft', () => {
  for (const status of ['starting', 'running', 'working', 'waiting']) {
    const chat = { status }
    assert.deepEqual(chatSendActions(chat, { reachable: true }), { runAction: 'interrupt', showSend: false })
    assert.deepEqual(chatSendActions(chat, { reachable: true, hasDraft: true }), { runAction: 'interrupt', showSend: true })
  }
})

test('only reachable chats with a conversation to resume replace Send with Resume', () => {
  for (const status of ['stopped', 'failed']) {
    assert.deepEqual(chatSendActions({ status, resumable: true }, { reachable: true, hasDraft: true }),
      { runAction: 'resume', showSend: false })
    assert.deepEqual(chatSendActions({ status }, { reachable: true }), { runAction: null, showSend: true })
  }
  for (const status of ['idle', 'running', 'stopped']) {
    assert.deepEqual(chatSendActions({ status, resumable: true }, { reachable: false, hasDraft: true }),
      { runAction: null, showSend: true })
  }
  assert.deepEqual(chatSendActions({ status: 'idle' }, { reachable: true }), { runAction: null, showSend: true })
})

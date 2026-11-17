import test from 'node:test'
import assert from 'node:assert/strict'
import { scopedComposerKey, composerChatId } from './composer-scope.js'
import { recoverDraft, consumeRecoveredDraft } from './composer-recovery.js'
import { attachments, clearAttachments } from './attachments.js'
import { sharedWith, chatMade, forgetShares } from './browser-share.js'

test('split New chat drafts and their browser shares have separate identities', () => {
  const first = scopedComposerKey('new', 'left'), second = scopedComposerKey('new', 'right')
  assert.notEqual(first, second)
  assert.equal(composerChatId(first), 'new')
  forgetShares()
  const told = []
  sharedWith(first, id => told.push(id))
  chatMade('chat-right', id => told.push(id), second)
  assert.deepEqual(told, [])
  chatMade('chat-left', id => told.push(id), first)
  assert.deepEqual(told, ['chat-left'])
  sharedWith(scopedComposerKey('chat-left', 'left'), id => told.push(id))
  assert.deepEqual(told, ['chat-left', 'chat-left'])
})

test('a failed first send recovers text and ready attachments in only its destination composer', () => {
  const key = scopedComposerKey('chat-retry', 'left')
  clearAttachments(key)
  recoverDraft(key, 'Please review', [{ id: 'old-id', kind: 'document', status: 'ready', name: 'brief.md', text: 'Important reference' }])
  assert.equal(consumeRecoveredDraft(scopedComposerKey('chat-retry', 'right')), '')
  assert.equal(consumeRecoveredDraft(key), 'Please review')
  assert.equal(consumeRecoveredDraft(key), '')
  assert.equal(attachments(key)[0].text, 'Important reference')
  assert.notEqual(attachments(key)[0].id, 'old-id')
  clearAttachments(key)
})

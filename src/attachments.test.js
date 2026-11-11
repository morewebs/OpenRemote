// What waits to ride a composer's next message, per composer - the store the
// browser's screenshot-to-composer will add to (addAttachment with no key
// lands in the focused composer).

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  addAttachment,
  attachments,
  clearAttachments,
  focusedComposer,
  moveAttachments,
  removeAttachment,
  setFocusedComposer,
  subscribe,
} from './attachments.js'

test('an attachment with no key lands in the focused composer, and listeners hear it', () => {
  let heard = 0
  const stop = subscribe(() => heard++)
  setFocusedComposer('chat-1')
  assert.equal(focusedComposer(), 'chat-1')
  const id = addAttachment({ kind: 'image', name: 'shot', mime: 'image/png', dataUrl: 'data:image/png;base64,AAAA' })
  assert.deepEqual(attachments('chat-1').map((a) => a.id), [id])
  removeAttachment('chat-1', id)
  assert.deepEqual(attachments('chat-1'), [])
  assert.equal(heard, 2)
  stop()
  setFocusedComposer(null)
  assert.equal(focusedComposer(), 'new', 'no composer named means the home screen')
})

test("the home screen's attachments move to the chat it starts", () => {
  clearAttachments('new')
  addAttachment({ kind: 'image', name: 'a' }, 'new')
  moveAttachments('new', 'chat-2')
  assert.deepEqual(attachments('new'), [])
  assert.deepEqual(attachments('chat-2').map((a) => a.name), ['a'])
  clearAttachments('chat-2')
})

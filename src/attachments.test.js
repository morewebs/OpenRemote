// What waits to ride a composer's next message, per composer - the store the
// browser's screenshot-to-composer will add to (addAttachment with no key
// lands in the focused composer).

import assert from 'node:assert/strict'
import test from 'node:test'
import { errorCopy } from './error-copy.js'
import {
  NO_COMPOSER,
  addAttachment,
  addToFocused,
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

test('a composer that goes ends its claim, so a picture added later is refused, not hidden in a closed chat', () => {
  const release = setFocusedComposer('chat-3')
  assert.deepEqual(addToFocused({ kind: 'image', name: 'one' }).key, 'chat-3')
  release()
  assert.equal(focusedComposer(), null, 'no composer holds the claim once its own closed')
  assert.throws(() => addToFocused({ kind: 'image', name: 'two' }), (err) => {
    assert.equal(err.voice, 'console')
    assert.equal(errorCopy(err), `The console could not do this: ${NO_COMPOSER}.`)
    return true
  })
  // The closed chat's composer got only what was added while it was open.
  assert.deepEqual(attachments('chat-3').map((a) => a.name), ['one'])
  clearAttachments('chat-3')
})

test("a release is only its own claim's: the composer that took over keeps it", () => {
  const first = setFocusedComposer('chat-4')
  const second = setFocusedComposer('chat-5')
  first()
  assert.equal(focusedComposer(), 'chat-5')
  const { key } = addToFocused({ kind: 'image', name: 'x' })
  assert.equal(key, 'chat-5')
  second()
  assert.equal(focusedComposer(), null)
  clearAttachments('chat-5')
})

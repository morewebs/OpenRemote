import test from 'node:test'
import assert from 'node:assert/strict'
import { createDraftStore } from './composer-drafts.js'

const storage = () => {
  const values = new Map()
  return { getItem: key => values.get(key), setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key) }
}

test('drafts survive store recreation and stay isolated per chat and pane', () => {
  const disk = storage()
  const drafts = createDraftStore(disk)
  drafts.write('new', 'unfinished task')
  drafts.write('chat', 'follow-up')
  drafts.write('chat::split', 'independent follow-up')
  const reloaded = createDraftStore(disk)
  assert.equal(reloaded.read('new'), 'unfinished task')
  assert.equal(reloaded.read('chat'), 'follow-up')
  assert.equal(reloaded.read('chat::split'), 'independent follow-up')
  drafts.write('chat', '')
  assert.equal(createDraftStore(disk).read('chat'), '')
})

test('late acknowledgement clears only the sent revision, even if later text is identical', () => {
  const drafts = createDraftStore(storage())
  drafts.write('chat', 'send this')
  const sent = drafts.snapshot('chat')
  drafts.write('chat', 'a new thought')
  drafts.acknowledge('chat', sent)
  assert.equal(drafts.read('chat'), 'a new thought')
  drafts.write('chat', 'send this')
  drafts.acknowledge('chat', sent)
  assert.equal(drafts.read('chat'), 'send this')
  drafts.acknowledge('chat', drafts.snapshot('chat'))
  assert.equal(drafts.read('chat'), '')
})

test('denied storage keeps in-memory drafts and notifies subscribers', () => {
  const drafts = createDraftStore({ getItem() { throw Error() }, setItem() { throw Error() }, removeItem() { throw Error() } })
  let changes = 0
  const unsubscribe = drafts.subscribe(() => changes++)
  drafts.write('new', 'still typing')
  assert.equal(drafts.read('new'), 'still typing')
  assert.equal(changes, 1)
  unsubscribe()
  drafts.write('new', '')
  assert.equal(changes, 1)
})

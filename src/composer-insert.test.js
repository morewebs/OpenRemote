import test from 'node:test'
import assert from 'node:assert/strict'
import { claimInserter, insertToComposer, resetInserter } from './composer-insert.js'

test('insert with no composer claimed returns false', () => {
  resetInserter()
  assert.equal(insertToComposer('hello'), false)
})

test('insert reaches the claiming composer', () => {
  resetInserter()
  const seen = []
  claimInserter((text) => seen.push(text))
  assert.equal(insertToComposer('hello'), true)
  assert.deepEqual(seen, ['hello'])
})

test('the latest claim wins', () => {
  resetInserter()
  const first = []
  const second = []
  claimInserter((text) => first.push(text))
  claimInserter((text) => second.push(text))
  insertToComposer('hi')
  assert.deepEqual(first, [])
  assert.deepEqual(second, ['hi'])
})

test('a claim dies with its composer: the pane is told nothing is open', () => {
  resetInserter()
  const seen = []
  const release = claimInserter((text) => seen.push(text))
  assert.equal(insertToComposer('first'), true)
  // The composer unmounts: a chat closed, another view opened.
  release()
  assert.equal(insertToComposer('after it closed'), false)
  assert.deepEqual(seen, ['first'], 'a composer that is gone is never written to')
})

test('releasing an old claim leaves the newer claimant holding the bus', () => {
  resetInserter()
  const first = []
  const second = []
  const releaseFirst = claimInserter((text) => first.push(text))
  const releaseSecond = claimInserter((text) => second.push(text))
  // The first composer unmounts after the second one took the bus.
  releaseFirst()
  assert.equal(insertToComposer('hi'), true)
  assert.deepEqual(second, ['hi'])
  assert.deepEqual(first, [])
  // The second one's own release is what clears it.
  releaseSecond()
  assert.equal(insertToComposer('again'), false)
})

test('claiming again from the same composer leaves one live claim', () => {
  resetInserter()
  const seen = []
  // A focus takes the bus a second time; the composer drops the claim it held.
  const first = claimInserter((text) => seen.push(text))
  const second = claimInserter((text) => seen.push(text))
  first()
  assert.equal(insertToComposer('hi'), true)
  assert.deepEqual(seen, ['hi'], 'one insert, not two')
  second()
  assert.equal(insertToComposer('again'), false)
})

test('reset forgets the claim', () => {
  resetInserter()
  claimInserter(() => {
    throw new Error('should not be called')
  })
  resetInserter()
  assert.equal(insertToComposer('hi'), false)
})

test('the claim names its composer, so the pane knows which chat it shared with', async () => {
  const { inserterKey } = await import('./composer-insert.js')
  resetInserter()
  assert.equal(inserterKey(), null)
  const release = claimInserter(() => {}, 'chat-1')
  assert.equal(inserterKey(), 'chat-1')
  claimInserter(() => {})
  assert.equal(inserterKey(), null, 'a claim without a key names none')
  release()
  resetInserter()
  const again = claimInserter(() => {}, 'new')
  assert.equal(inserterKey(), 'new')
  again()
  assert.equal(inserterKey(), null, 'a released claim names nothing')
})

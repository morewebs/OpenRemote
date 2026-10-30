import assert from 'node:assert/strict'
import test from 'node:test'
import { GREETINGS, greetingAt, greetingForNewChat, nextGreetingIndex } from './greetings.js'

test('the greeting walks the list and wraps', () => {
  assert.equal(nextGreetingIndex(-1), 0)
  assert.equal(nextGreetingIndex(0), 1)
  assert.equal(nextGreetingIndex(GREETINGS.length - 1), 0)
  assert.equal(greetingAt(2), GREETINGS[2])
  assert.equal(greetingAt(99), GREETINGS[0])
})

test('a new chat takes the next sentence and remembers it', () => {
  const store = new Map()
  const storage = {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => store.set(key, value),
  }
  const first = greetingForNewChat(storage)
  const second = greetingForNewChat(storage)
  assert.equal(first, 0)
  assert.equal(second, 1)
  assert.notEqual(greetingAt(first), greetingAt(second))
})

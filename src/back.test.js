import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createBackStack } from './back.js'

// A history that pops synchronously, like the browser's once popstate fires.
function fakeHistory() {
  const entries = [null]
  let index = 0
  let stack
  const h = {
    get state() {
      return entries[index]
    },
    pushState(state) {
      entries.splice(index + 1, Infinity, state)
      index += 1
    },
    back() {
      if (index === 0) {
        h.left = true
        return
      }
      index -= 1
      stack.onPop(entries[index])
    },
    left: false,
    get length() {
      return index + 1
    },
  }
  return {
    history: h,
    bind(s) {
      stack = s
    },
  }
}

test('back closes the newest open thing first', () => {
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  stack.push(() => closed.push('view'))
  stack.push(() => closed.push('modal'))
  history.back()
  assert.deepEqual(closed, ['modal'])
  history.back()
  assert.deepEqual(closed, ['modal', 'view'])
  history.back()
  assert.equal(history.left, true, 'with nothing open, back leaves the app')
})

test('something closed another way leaves no dead back press', () => {
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  stack.push(() => closed.push('view'))
  const release = stack.push(() => closed.push('picker'))
  release()
  history.back()
  assert.deepEqual(closed, ['view'], 'one press skips the spent picker entry')
})

test('pushes after a release stack above it', () => {
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  stack.push(() => closed.push('drawer'))
  const release = stack.push(() => closed.push('menu'))
  release()
  stack.push(() => closed.push('modal'))
  history.back()
  history.back()
  assert.deepEqual(closed, ['modal', 'drawer'])
})

test('off the phone it does nothing', () => {
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history, { enabled: false })
  bind(stack)
  stack.push(() => {})
  assert.equal(history.length, 1)
})

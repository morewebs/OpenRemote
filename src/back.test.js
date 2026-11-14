import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createBackStack } from './back.js'

// A history with immediate pops or queued browser-style traversals.
function fakeHistory({ async = false } = {}) {
  const entries = [null]
  let index = 0
  let stack
  const pops = []
  const h = {
    get state() {
      return entries[index]
    },
    pushState(state) {
      entries.splice(index + 1, Infinity, state)
      index += 1
    },
    back() {
      const pop = () => {
        if (index === 0) {
          h.left = true
          return
        }
        index -= 1
        stack.onPop(entries[index])
      }
      if (async) pops.push(pop)
      else pop()
    },
    flush() {
      while (pops.length) pops.shift()()
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

test('opening Settings while the drawer is closing keeps its own Back entry', () => {
  const { history, bind } = fakeHistory({ async: true })
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  const closeDrawer = stack.push(() => closed.push('drawer'))
  closeDrawer()
  stack.push(() => closed.push('settings'))
  history.flush()
  assert.deepEqual(closed, [], 'the pending drawer pop must not close Settings')
  history.back()
  history.flush()
  assert.deepEqual(closed, ['settings'], 'one Back must close Settings')
  assert.equal(history.left, false, 'Back must stay on the original home')
})

test('a popup closed before the drawer finishes closing leaves no Back entry', () => {
  const { history, bind } = fakeHistory({ async: true })
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  const closeDrawer = stack.push(() => closed.push('drawer'))
  closeDrawer()
  const closeSettings = stack.push(() => closed.push('settings'))
  closeSettings()
  history.flush()
  assert.equal(history.length, 1)
  history.back()
  history.flush()
  assert.deepEqual(closed, [])
  assert.equal(history.left, true)
})

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

test('closing the newest thing takes its history entry with it', () => {
  // A release that keeps its entry is a back press with nothing to show for
  // it: the press is spent stepping over a thing that is already gone, and
  // the press after it leaves the app from a screen the user believes is
  // closed. The entry goes when the thing closes.
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  const release = stack.push(() => closed.push('drawer'))
  assert.equal(history.length, 2, 'opening holds one entry')
  release()
  assert.deepEqual(closed, [], 'closing by hand runs no handler')
  assert.equal(history.length, 1, 'and gives the entry back')

  // Nothing is open, so the next press is the one that leaves.
  history.back()
  assert.equal(history.left, true)
})

test('three opens and closes leave one press to leave, not four', () => {
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  for (const name of ['menu', 'picker', 'modal']) {
    const release = stack.push(() => closed.push(name))
    release()
  }
  assert.equal(history.length, 1, 'every closed thing gave its entry back')
  history.back()
  assert.deepEqual(closed, [], 'no spent entry was mistaken for something to close')
  assert.equal(history.left, true, 'the first press with nothing open leaves')
})

test('closing the top of two does not close the one below', () => {
  // The pop this module asks for lands on the lower thing's entry. Reading
  // that popstate as the user's press would close what is still on screen.
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  stack.push(() => closed.push('drawer'))
  const release = stack.push(() => closed.push('menu'))
  release()
  assert.deepEqual(closed, [], 'closing the top leaves the drawer open')
  assert.equal(history.length, 2, 'the drawer still holds its entry')

  history.back()
  assert.deepEqual(closed, ['drawer'], 'and the next press is the drawer\'s')
})

test('a spent entry below a live one is stepped over without closing it', () => {
  const { history, bind } = fakeHistory()
  const stack = createBackStack(history)
  bind(stack)
  const closed = []
  const releaseDrawer = stack.push(() => closed.push('drawer'))
  stack.push(() => closed.push('modal'))
  // Close the lower one first: it is not the newest, so its entry stays
  // until the pop above takes it.
  releaseDrawer()
  history.back()
  assert.deepEqual(closed, ['modal'], 'the live modal closes, not the spent drawer')
  assert.equal(history.length, 2, 'the drawer entry is still one below')
  // The next press steps over the spent entry without running anything.
  history.back()
  assert.deepEqual(closed, ['modal'], 'the spent entry never runs its handler')
  history.back()
  assert.equal(history.left, true, 'and the press after that leaves the app')
})

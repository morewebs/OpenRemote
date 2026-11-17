import assert from 'node:assert/strict'
import test from 'node:test'
import { createEscLayers } from './esc-layer.js'

test('workspace Escape takes priority over interrupt regardless of window listener order', () => {
  const layers = createEscLayers()
  assert.equal(layers.blocksShortcut('Esc'), false)
  const workspace = layers.claim()
  let interrupts = 0
  if (!layers.blocksShortcut('Esc')) interrupts++
  assert.equal(interrupts, 0)
  assert.equal(layers.blocksShortcut('Mod+1'), false)
  workspace.release()
  assert.equal(layers.blocksShortcut('Esc'), false)
})

test('only the top layer counts as open', () => {
  const layers = createEscLayers()
  const window_ = layers.claim() // a view in its own window
  const confirm = layers.claim() // a delete confirm opened over it
  assert.equal(window_.isTop(), false)
  assert.equal(confirm.isTop(), true)
  // The confirm closes: the window under it is the top one again.
  confirm.release()
  assert.equal(window_.isTop(), true)
  window_.release()
  assert.equal(layers.depth(), 0)
})

test('an older layer unmounting never lifts a newer claim', () => {
  const layers = createEscLayers()
  const rule = layers.claim() // the automation form
  const endpoints = layers.claim() // Manage endpoints over it
  rule.release() // the form goes away first
  assert.equal(endpoints.isTop(), true, 'the dialog on screen still answers Esc')
  assert.equal(layers.depth(), 1)
  endpoints.release()
  assert.equal(layers.depth(), 0)
})

test('one Esc closes one layer, however the listeners are ordered', () => {
  const layers = createEscLayers()
  const closed = []
  // An overlay as the hook signs it up: a claim, and a window listener that
  // only answers when it is the top one.
  const overlay = (name) => {
    const claim = layers.claim()
    return {
      claim,
      onKey: (e) => {
        if (e.defaultPrevented) return
        if (!claim.isTop()) return
        e.defaultPrevented = true
        closed.push(name)
      },
    }
  }
  const settings = overlay('settings')
  const confirm = overlay('confirm')
  // A real press reaches every listener, in the order they were added: the
  // window first, then the confirm it opened.
  const press = () => {
    const e = { defaultPrevented: false }
    for (const layer of [settings, confirm]) layer.onKey(e)
    return e
  }
  assert.equal(press().defaultPrevented, true, 'the key is consumed')
  assert.deepEqual(closed, ['confirm'], 'the layer underneath stays open')
  // The confirm is gone; the next press closes the window.
  confirm.claim.release()
  press()
  assert.deepEqual(closed, ['confirm', 'settings'])
})

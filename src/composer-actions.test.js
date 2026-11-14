import test from 'node:test'
import assert from 'node:assert/strict'
import { composerActions, beginCompletion } from './composer-actions.js'

test('the Add menu keeps documents and references when an App cannot read images', () => {
  const ids = composerActions({ images: false, mobile: true }).map(a => a.id)
  assert.ok(ids.includes('files'))
  assert.ok(ids.includes('reference'))
  assert.ok(ids.includes('commands'))
  assert.ok(!ids.includes('photos'))
  assert.ok(!ids.includes('camera'))
})

test('camera and clipboard actions appear only where available, plugins retain identity', () => {
  const desktop = composerActions({ images: true, mobile: false }).map(a => a.id)
  assert.ok(!desktop.includes('camera'))
  assert.ok(!desktop.includes('clipboard'))
  const phone = composerActions({ images: true, mobile: true, clipboard: true, pluginActions: [{ id: 'voice/dictate', name: 'Dictate', pluginName: 'Voice', state: 'busy' }], effort: 'high' })
  assert.deepEqual(phone.filter(a => a.tile).map(a => a.id), ['camera', 'photos', 'files'])
  assert.ok(phone.some(a => a.id === 'clipboard'))
  assert.equal(phone.find(a => a.id === 'plugin:voice/dictate').detail, 'Voice')
  assert.ok(phone.some(a => a.id === 'effort'))
  assert.ok(!phone.some(a => a.id === 'web-search'), 'no fictitious built-in mode')
})

test('menu completion inserts at the selected cursor without destroying the draft', () => {
  assert.deepEqual(beginCompletion('Review this file', 7, 11, '@'), { text: 'Review @ file', caret: 8 })
  assert.deepEqual(beginCompletion('Hello', 5, 5, '/'), { text: 'Hello /', caret: 7 })
  assert.deepEqual(beginCompletion('', 0, 0, '@'), { text: '@', caret: 1 })
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { pictureKey, pictureMissing } from './chat-pictures.js'

test('a picture is keyed by the device asked, the chat route and its hash', () => {
  assert.equal(pictureKey('', null, 'abc'), '||abc')
  assert.notEqual(pictureKey('/devices/d1', 's1', 'abc'), pictureKey('', null, 'abc'))
})

test('a picture that could not be fetched says why, the 16 MiB answer cap included', () => {
  assert.match(pictureMissing({ status: 413 }, true), /too large to bring over/)
  assert.match(pictureMissing({ status: 404 }, false), /no longer kept/)
  assert.match(pictureMissing(new Error('Failed to fetch'), true), /can't be reached/)
  assert.match(pictureMissing(new Error('Failed to fetch'), false), /could not load/)
})

// The web console's fetch over the encrypted link answers in fetch's shape,
// pictures included: the API client's blob(sha) reads `response.blob()`.

import assert from 'node:assert/strict'
import test from 'node:test'
import { shaped } from './rpc.js'

test('an answer reads as text, JSON and a typed blob', async () => {
  const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47])
  const picture = shaped({ status: 200, contentType: 'image/png', body: bytes })
  assert.equal(picture.ok, true)
  assert.equal(picture.headers.get('Content-Type'), 'image/png')
  const blob = await picture.blob()
  assert.equal(blob.type, 'image/png')
  assert.deepEqual(new Uint8Array(await blob.arrayBuffer()), bytes)

  const json = shaped({ status: 404, contentType: 'application/json', body: new TextEncoder().encode('{"error":"gone"}') })
  assert.equal(json.ok, false)
  assert.deepEqual(await json.json(), { error: 'gone' })
  assert.equal((await shaped({ status: 200, contentType: null, body: new Uint8Array(0) }).blob()).type, '')
})

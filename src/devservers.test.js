import test from 'node:test'
import assert from 'node:assert/strict'
import { findDevServers } from './devservers.js'

test('dev servers are found in a reply, once each, with the sentence around them left out', () => {
  assert.deepEqual(
    findDevServers('Ready on http://0.0.0.0:3000. Also http://localhost:5173/app, and http://localhost:5173/app').map((d) => d.url),
    ['http://localhost:3000', 'http://localhost:5173/app'],
  )
  assert.deepEqual(findDevServers('see http://myapp.localhost:8080/x)').map((d) => d.url), ['http://myapp.localhost:8080/x'])
  assert.deepEqual(findDevServers('no port http://localhost/ and https://example.com:3000'), [])
  assert.deepEqual(findDevServers(null), [])
})

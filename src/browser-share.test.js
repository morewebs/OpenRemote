// A page, its picture or a picked element shared with a chat from the
// browser puts the browser in play for that chat (the daemon's
// /sessions/{id}/browser/shared): its agent is listed the browser tools.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { chatMade, forgetShares, sharedWith } from './browser-share.js'

const read = (p) => readFileSync(new URL(p, import.meta.url), 'utf8')

test("a share with a running chat tells the daemon that chat's id", () => {
  forgetShares()
  const told = []
  sharedWith('chat-1', (id) => told.push(id))
  assert.deepEqual(told, ['chat-1'])
  sharedWith(null, (id) => told.push(id))
  assert.deepEqual(told, ['chat-1'], 'no composer, nothing to tell')
})

test('a share with the new chat waits for the chat it becomes', () => {
  forgetShares()
  const told = []
  const tell = (id) => told.push(id)
  chatMade('quiet', tell)
  assert.deepEqual(told, [], 'a chat made without a share is not told')
  sharedWith('new', tell)
  assert.deepEqual(told, [], 'no chat exists yet')
  chatMade('chat-2', tell)
  assert.deepEqual(told, ['chat-2'])
  chatMade('chat-3', tell)
  assert.deepEqual(told, ['chat-2'], 'once only')
})

test('the browser tells the daemon on a pick or a picture, and a new chat on its making', () => {
  const share = read('./browser-share.js')
  assert.match(share, /browserCall\('POST', `\/sessions\/\$\{encodeURIComponent\(id\)\}\/browser\/shared`/)
  const view = read('./BrowserView.jsx')
  assert.match(view, /const target = inserterKey\(\)\s*if \(insertToComposer\(line\)\) \{\s*sharedWith\(target, tellDaemon\)/)
  assert.match(view, /const \{ key \} = addToFocused\(item\)\s*sharedWith\(key, tellDaemon\)/)
  const fresh = read('./NewChat.jsx')
  assert.match(fresh, /claimInserter\(attach, 'new'\)/)
  const made = fresh.indexOf('const id = await createChat(')
  const told = fresh.indexOf('chatMade(id, tellDaemon)')
  assert.ok(made > 0 && told > made && told < fresh.indexOf('onOpen(id)', made), 'told once the chat exists, before it opens')
  const chat = read('./ChatView.jsx')
  assert.match(chat, /claimInserter\(attach, composerKey\)/)
})

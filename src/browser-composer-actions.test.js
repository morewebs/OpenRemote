import test from 'node:test'
import assert from 'node:assert/strict'
import { claimInserter, composerTarget, resetInserter, advanceComposerDraft } from './composer-insert.js'
import { browserContextActions, collectBrowserContext } from './browser-composer-actions.js'

test('browser context is offered from reported capabilities, not optimism', () => {
  assert.deepEqual(browserContextActions({ features: null, images: true }).map(i => i.id), ['link'])
  assert.deepEqual(browserContextActions({ features: { cdp: true }, images: false }).map(i => i.id), ['text', 'link', 'element'])
  assert.deepEqual(browserContextActions({ features: { cdp: true }, images: true }).map(i => i.id), ['text', 'link', 'screenshot', 'region', 'element'])
})

test('a captured browser recipient stays with its initiating split composer and dies on unmount', () => {
  resetInserter()
  const first = [], second = []
  const release = claimInserter(t => first.push(t), 'first')
  const target = composerTarget('first')
  claimInserter(t => second.push(t), 'second')
  assert.equal(target.insert('page'), true)
  assert.deepEqual(first, ['page'])
  assert.deepEqual(second, [])
  release()
  claimInserter(t => first.push(t), 'first')
  assert.equal(target.isLive(), false, 'reopening the same chat is a new recipient')
  assert.equal(target.insert('stale'), false)
})

test('browser collection returns real page text with provenance and refuses missing replies', async () => {
  const tab = { native: 'tab-1', title: 'Example', url: 'https://example.test' }
  const seen = []
  const item = await collectBrowserContext('text', tab, async (command, args) => {
    seen.push({ command, args })
    return { result: { value: 'Hello from the actual page' } }
  })
  assert.equal(item.kind, 'document')
  assert.match(item.text, /https:\/\/example.test[\s\S]*Hello from the actual page/)
  assert.equal(seen[0].command, 'browser_cdp')
  await assert.rejects(collectBrowserContext('text', tab, async () => null), /read|reach/i)
  await assert.rejects(collectBrowserContext('screenshot', tab, async () => null), /picture|screenshot/i)
})

test('sending invalidates old clipboard or browser picks while the composer stays mounted', () => {
  resetInserter()
  claimInserter(() => {}, 'same-chat')
  const target = composerTarget('same-chat')
  advanceComposerDraft('same-chat')
  assert.equal(target.isLive(), false)
  assert.equal(composerTarget('same-chat').isLive(), true)
})

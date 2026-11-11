// How OpenRemote's browser tools read in a chat's cards and approvals.

import assert from 'node:assert/strict'
import test from 'node:test'
import { browserTool, foldsResult, isBrowserTool } from './browser-tools.js'

const P = 'mcp__openremote__browser_'

test('only the openremote browser tools are browser tools', () => {
  assert.ok(isBrowserTool(`${P}click`))
  assert.ok(!isBrowserTool('mcp__playwright__browser_click'))
  assert.ok(!isBrowserTool('bash'))
  assert.ok(!isBrowserTool(undefined))
})

test('a call reads as "Browser · <verb>" with the element and the address', () => {
  const facts = browserTool(`${P}click`, { element: 'Sign in button', url: 'https://a.test/' })
  assert.equal(facts.label, 'Browser · click')
  assert.equal(facts.description, 'Sign in button')
  assert.equal(facts.url, 'https://a.test/')
  assert.equal(facts.detail, null)
  assert.equal(browserTool(`${P}snapshot`, {}).label, 'Browser · read page')
  assert.equal(browserTool(`${P}tab_open`, {}).label, 'Browser · open tab')
  assert.equal(browserTool(`${P}go`, { direction: 'back' }).label, 'Browser · back')
})

test('the argument worth reading before saying yes is shown, never a JSON dump', () => {
  assert.equal(browserTool(`${P}type`, { text: 'hello', submit: true }).detail, '"hello", then Enter')
  assert.equal(browserTool(`${P}press`, { key: 'Enter' }).detail, 'Enter')
  const script = browserTool(`${P}evaluate`, { script: 'document.title' })
  assert.equal(script.detail, 'document.title')
  assert.equal(script.mono, true)
  assert.equal(browserTool(`${P}upload`, { paths: ['a.txt', 'b.txt'] }).detail, 'a.txt\nb.txt')
  assert.equal(browserTool(`${P}wait`, { text: 'Done' }).detail, 'until "Done" shows')
  assert.equal(browserTool(`${P}viewport`, { width: 390, height: 844 }).detail, '390 × 844')
  assert.equal(browserTool(`${P}dialog`, { accept: false }).detail, 'dismiss')
  assert.equal(browserTool(`${P}scroll`, { dy: -200 }).detail, 'up 200 px')
})

test('a page read as text folds; other results do not', () => {
  assert.ok(foldsResult(`${P}snapshot`))
  assert.ok(!foldsResult(`${P}click`))
  assert.ok(!foldsResult('agents_start'))
})

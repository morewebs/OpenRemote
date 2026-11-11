// Agents in tabs (ported from the browser branch's 0d3211a): a tab a chat's
// agent opens joins the strip without taking the view, wears a breathing
// badge with the chat's title, and an info bar lets the user take over,
// hand back or stop sharing (the daemon's `/browser/tabs/{tab}/control`).

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { closePage, focusTab, getState, onTab, setState } from './browser-store.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

const agent = { kind: 'agent', chat: 'c1', title: 'Fix the login' }

test("an agent's tab joins the strip at the end, and the user's view stays where it was", () => {
  setState(() => ({ tabs: [{ key: 'u', native: 'tab-1', url: 'https://mine.test/' }], active: 'u' }))
  onTab({ id: 'tab-7', url: 'https://agent.test/', title: 'Agent page', controller: agent })
  const { tabs, active } = getState()
  assert.equal(active, 'u')
  assert.deepEqual(tabs.map((t) => t.native), ['tab-1', 'tab-7'])
  assert.deepEqual(tabs[1].controller, agent)
  // A later word from the shell updates it in place; null clears the hold.
  onTab({ id: 'tab-7', url: 'https://agent.test/', title: 'Agent page', controller: null })
  assert.equal(getState().tabs[1].controller, null)
})

test("a popup from the user's own page is shown; one from an agent's tab is not", () => {
  setState(() => ({ tabs: [{ key: 'u', native: 'tab-1', url: 'https://mine.test/' }], active: 'u' }))
  onTab({ id: 'tab-2', opener: 'tab-1', url: 'https://pop.test/' })
  assert.equal(getState().tabs.find((t) => t.key === getState().active).native, 'tab-2')
  setState(() => ({ active: 'u' }))
  onTab({ id: 'tab-3', opener: 'tab-1', url: 'https://agentpop.test/', controller: agent })
  assert.equal(getState().active, 'u')
  assert.deepEqual(getState().tabs.map((t) => t.native), ['tab-1', 'tab-3', 'tab-2'])
})

test('a page the console closed never comes back on a late event', () => {
  setState(() => ({ tabs: [{ key: 'u', native: 'tab-1' }], active: 'u' }))
  closePage('tab-40')
  onTab({ id: 'tab-40', url: 'https://late.test/', controller: agent })
  assert.ok(!getState().tabs.some((t) => t.native === 'tab-40'))
})

test("a wish to show an agent's tab waits for it to join the strip", () => {
  setState(() => ({ tabs: [{ key: 'u', native: 'tab-1' }], active: 'u' }))
  assert.equal(focusTab('tab-50'), false)
  onTab({ id: 'tab-50', url: 'https://agent.test/', controller: agent })
  assert.equal(getState().tabs.find((t) => t.key === getState().active).native, 'tab-50')
})

test('the badge, the take-over bar and the control route are wired', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /\{t\.controller && <AgentBadge controller=\{t\.controller\} \/>\}/)
  assert.match(view, /\{current\?\.controller && live && <AgentBar controller=\{current\.controller\} onControl=\{control\} \/>\}/)
  assert.match(view, /browserCall\('POST', `\/browser\/tabs\/\$\{encodeURIComponent\(tab\.native\)\}\/control`, \{ action \}\)/)
  assert.match(view, /patchTab\(tab\.key, \{ controller: got\?\.controller \?\? null \}\)/)
  assert.match(view, /notify\(errorCopy\(err\)\)/)
  const bar = read('./BrowserInfobar.jsx')
  for (const action of ['take_over', 'hand_back', 'release']) assert.match(bar, new RegExp(`onControl\\('${action}'\\)`))
  assert.match(bar, /Take over/)
  assert.match(bar, /className="bw-agent-name pv"/)
  const css = read('./browser.css')
  assert.match(css, /\.bw-agent\.live \.bw-agent-dot,\s+\.bw-agent-dot\.live \{\s+background: var\(--green\);\s+animation: bw-breathe/)
  assert.match(css, /html\[data-reduce-motion\] \.bw-agent\.live \.bw-agent-dot/)
  // The daemon route the bar calls exists, with these three actions.
  const daemon = read('../daemon/crates/openremote-daemon/src/browser/mod.rs')
  for (const action of ['"take_over"', '"hand_back"', '"release"']) assert.ok(daemon.includes(action), action)
})

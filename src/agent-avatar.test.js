import assert from 'node:assert/strict'
import test from 'node:test'
import { avatarInitials, defaultAgentAvatar } from './agent-avatar.js'

test('initials use the first and last words in the title', () => {
  assert.equal(avatarInitials('  Review: the sidebar!  '), 'RS')
  assert.equal(avatarInitials('Codex'), 'C')
  assert.equal(avatarInitials('24-hour uptime'), '2U')
  assert.equal(avatarInitials('...'), '?')
})

test('initials keep Unicode graphemes together and use at most two', () => {
  assert.equal(avatarInitials('E\u0301lan A\u030Aland'), '\u00c9\u00c5')
  assert.equal(avatarInitials('\u00dfeta codex'), 'SC')
  assert.equal(avatarInitials('\u6771\u4eac \u6771\u6d0b'), '\u6771\u6771')
  assert.equal(avatarInitials('\u0915\u093f\u0924\u093e\u092c notes'), '\u0915\u093fN')
})

test('the color is stable across renames and uses the id, not the App or title', () => {
  const first = defaultAgentAvatar('chat-a', 'Review the sidebar')
  assert.deepEqual(first, { initials: 'RS', color: 'teal' })
  assert.deepEqual(defaultAgentAvatar('chat-a', 'Review the sidebar'), first)
  const renamed = defaultAgentAvatar('chat-a', 'Review docs')
  assert.equal(renamed.color, first.color)
  assert.equal(renamed.initials, 'RD')
  assert.notEqual(defaultAgentAvatar('chat-b', 'Review the sidebar').color, first.color)
})

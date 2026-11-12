// The first-day empty states, in the console's voice: each says what the
// thing is for, what to do next, and whether it is empty or still loading,
// and none of them renders a button whose first step is somewhere else. The
// copy lives in view components with no renderer here, so this reads the
// source text, the way the other wiring tests do.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('New chat says there is nothing to run a task on, and where the installer is', () => {
  const newchat = source('NewChat.jsx')
  // Only while the daemon is answering: an empty list from a starting daemon
  // is "not yet", not "nothing installed".
  assert.match(newchat, /connection\.state === 'connected' && !available\.length/)
  assert.match(newchat, /No App is installed on this computer yet, so a chat has nowhere to run\./)
  assert.match(newchat, /Install one from\s+the App picker under the box/)
  // A calm row: no border, no tint, and no button in it.
  assert.match(newchat, /className="nc-first-run"/)
  assert.match(source('newchat.css'), /\.nc-first-run \{/)
  // One calm row, no second control: the picker under the box is the way in.
  const row = newchat.slice(newchat.indexOf('key="no-agent"'), newchat.indexOf('nc-box${needsSignIn'))
  assert.ok(!/<button/.test(row), 'the row renders no button')
})

test('Discover separates never fetched from fetching, and names the next step', () => {
  const discover = source('PluginDiscover.jsx')
  assert.match(discover, /No marketplace has answered yet\./)
  assert.match(discover, /Refresh one under Marketplaces to see its plugins/)
  assert.match(discover, /Reading the marketplaces' catalogs\.\.\./)
  // The two states are distinct branches, not one line with a word swapped.
  assert.match(discover, /all\.length === 0 && !fetching\.length/)
  assert.match(discover, /all\.length === 0 && fetching\.length > 0/)
})

test('Marketplaces with none says what a marketplace adds, and that nothing is broken', () => {
  const marketplaces = source('PluginMarketplaces.jsx')
  assert.match(marketplaces, /No marketplace has been added yet\./)
  assert.match(marketplaces, /Everything under Plugins works without one/)
})

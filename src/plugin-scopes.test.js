// Who a plugin is for: the words the install review and the installed rows
// use, the scope the review starts on, the request it sends, and the
// Plugins page's lists of plugins waiting for this computer's review. The
// components have no renderer here, so their wiring is pinned in the
// source, as the other plugin wiring tests pin it.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { DaemonApi } from './api/daemon.js'
import {
  SCOPE_HINT,
  SCOPE_LABEL,
  contributionLines,
  defaultScope,
  removeScopeNote,
  reviewWaitingLine,
  scopeLabel,
  sourceLabel,
} from './plugins.js'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('every scope the daemon offers has a label and a hint', () => {
  for (const scope of ['global', 'machine', 'project_local', 'project']) {
    assert.ok(SCOPE_LABEL[scope], scope)
    assert.ok(SCOPE_HINT[scope], scope)
  }
  // Every device's copy is reviewed there: the hint does not promise an
  // install the other devices never make on their own.
  assert.match(SCOPE_HINT.global, /review/i)
  assert.doesNotMatch(SCOPE_HINT.global, /installed on each/i)
})

test('an installed row names its project when it is for one', () => {
  assert.equal(scopeLabel({ scope: 'machine' }), 'This machine')
  assert.equal(scopeLabel({ scope: 'global' }), 'Every device')
  assert.equal(scopeLabel({ scope: 'project_local' }, 'site'), 'Project site')
  assert.equal(scopeLabel({ scope: 'project' }, 'site'), 'Project site, in its repo')
  assert.equal(scopeLabel({ scope: 'project' }, null), 'A project, in its repo')
})

test('the review starts on where the copy it replaces is, then where it came from', () => {
  const all = ['global', 'machine', 'project_local', 'project']
  assert.equal(defaultScope({ scopes: all }), 'machine')
  assert.equal(defaultScope({ scopes: all, installed_scope: 'project_local' }), 'project_local')
  assert.equal(defaultScope({ scopes: all, for_project: 'p1' }), 'project')
  assert.equal(defaultScope({ scopes: all, for_account: true }), 'global')
  // Only a scope the plugin allows: an App's plugin is never one project's.
  assert.equal(defaultScope({ scopes: ['global', 'machine'], for_project: 'p1' }), 'machine')
  assert.equal(defaultScope({ scopes: ['machine'], for_account: true }), 'machine')
  assert.equal(defaultScope(null), 'machine')
})

test('what a plugin adds reads a line per kind, and a harness is an App', () => {
  assert.deepEqual(contributionLines(null), [])
  assert.deepEqual(
    contributionLines({
      harnesses: [{ id: 'toy', name: 'Toy CLI' }],
      tools: [{ name: 'word_count' }, { name: 'slugify' }],
      composer_inputs: [{ label: 'Dictate' }],
      views: [{ title: 'Stats' }],
      settings: [{ label: 'API key' }],
    }),
    ['An App: Toy CLI', 'Agent tools: word_count, slugify', 'Composer button: Dictate', 'View: Stats', 'Setting: API key'],
  )
  assert.deepEqual(contributionLines({ harnesses: [{ name: 'A' }, { name: 'B' }] }), ['Apps: A, B'])
})

test('the remove button says when it reaches past this computer', () => {
  assert.match(removeScopeNote({ scope: 'project' }), /repo/)
  assert.match(removeScopeNote({ scope: 'global' }), /every device/)
  assert.equal(removeScopeNote({ scope: 'machine' }), null)
})

test('a plugin waiting for review says why, in its own line', () => {
  assert.equal(reviewWaitingLine({ why: 'new' }, 'site'), 'New in the repo of site')
  assert.equal(reviewWaitingLine({ why: 'changed' }, null), 'Changed in the repo since you approved it')
  assert.equal(reviewWaitingLine({ why: 'missing', kind: 'account' }), 'On your other devices, not here yet')
  assert.equal(reviewWaitingLine({ why: 'update', kind: 'account' }), 'A newer copy is on your other devices')
})

test('the install request carries who it is for, and only that', async () => {
  const seen = []
  const original = globalThis.fetch
  globalThis.fetch = async (url, init) => {
    seen.push({ url, init })
    return new Response('{"result":{}}', { status: 200, headers: { 'Content-Type': 'application/json' } })
  }
  try {
    const api = new DaemonApi('http://127.0.0.1:9', 'token')
    await api.installStagedPlugin('s1')
    await api.installStagedPlugin('s2', { scope: 'project', project: 'p1' })
    await api.installStagedPlugin('s3', { scope: 'global' })
  } finally {
    globalThis.fetch = original
  }
  const bodies = seen.map((s) => JSON.parse(s.init.body))
  assert.deepEqual(Object.keys(bodies[0]), ['request_id'])
  assert.equal(bodies[1].scope, 'project')
  assert.equal(bodies[1].project, 'p1')
  assert.equal(bodies[2].scope, 'global')
  assert.equal(bodies[2].project, undefined)
  for (const body of bodies) assert.equal(body.powers, undefined, 'a scope never names a power')
})

test('the review asks who it is for before Install, and a project scope needs its project', () => {
  const modal = source('InstallPluginModal.jsx')
  assert.match(modal, /SCOPE_LABEL\[s\]/)
  assert.match(modal, /installStagedPlugin\(staged\.stage_id, \{ scope, project: forProject \? project : undefined \}\)/)
  assert.match(modal, /disabled=\{busy \|\| \(forProject && !project\)\}/)
  // A repo's plugin or the account's copy is read at once, like a Discover entry.
  assert.match(modal, /else if \(source\) read\(source\)/)
})

test('the Plugins page lists what waits for review, each with a Review button', () => {
  const view = source('PluginsView.jsx')
  assert.match(view, /plugins\?\.found/)
  assert.match(view, /plugins\?\.global_pending/)
  assert.match(view, /onReview\(\{ kind: 'project', project: f\.project, name: f\.name \}\)/)
  assert.match(view, /onReview\(\{ kind: 'account', id: g\.id \}\)/)
  assert.match(view, /scopeLabel\(p, /)
  // An App appearing or leaving with its plugin reaches the pickers.
  const state = source('state/console.jsx')
  assert.match(state, /api\.installStagedPlugin\(stageId, where\)\s*\n\s*await Promise\.all\(\[refreshPlugins\(\), refreshCapabilities\(\)\]\)/)
})

test('a plugin another device removed is off here, and this device confirms the removal', () => {
  assert.match(reviewWaitingLine({ why: 'removed', kind: 'account' }), /^Removed on another device/)
  const view = source('PluginsView.jsx')
  assert.match(view, /g\.why === 'removed'/)
  assert.match(view, /run\(\(\) => removePlugin\(g\.id\)\)/)
})

test("an account plugin's source never names a path on another device", () => {
  assert.equal(sourceLabel({ kind: 'folder', path: '' }), 'A folder on another of your devices')
  assert.equal(sourceLabel({ kind: 'archive', location: '' }), 'An archive on another of your devices')
})

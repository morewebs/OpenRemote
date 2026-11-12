// The console calls a harness an App in everything a person reads. The code,
// the routes and the ids keep `harness` on purpose (it is what the daemon's
// API and the plugin contract call it), so this pins the user-facing half:
// one name for the thing on screen, and the same one in every place it is
// offered. A rename that lands in the tab but not in the picker is exactly
// the drift this reads for.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { SECTIONS, SETTINGS_ENTRIES } from './settings-search.js'
import { ACTIONS } from './shortcuts.js'
import { HARNESS_STOP_NAME } from './stop-harness.js'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the settings index says Apps, and keeps the old word searchable', () => {
  // The section keeps its id (`harnesses`), because a stored tab and the
  // view's own data-stg read it; only the label a person sees changed.
  const section = SECTIONS.find((s) => s.id === 'harnesses')
  assert.equal(section.label, 'Apps')
  assert.equal(SETTINGS_ENTRIES.find((e) => e.key === 'harnesses').label, 'Apps')
  assert.equal(SETTINGS_ENTRIES.find((e) => e.key === 'new-tasks-harness').label, 'App')
  // Somebody who learned the old word still finds the row.
  assert.ok(
    SETTINGS_ENTRIES.find((e) => e.key === 'new-tasks-harness').terms.includes('harness'),
    'the old word is still a search term',
  )
})

test('the surfaces a person meets say App', () => {
  assert.match(source('HarnessManagerModal.jsx'), /className="dv-modal-title">Apps</)
  assert.match(source('HarnessManagerModal.jsx'), /aria-label="Apps"/)
  assert.match(source('SettingsView.jsx'), /h2 className="stg-title">Apps</)
  assert.match(source('PluginsView.jsx'), />App</)
  assert.match(source('RaceModal.jsx'), />Apps</)
  assert.match(source('AgentsSection.jsx'), />Apps workers may use</)
  assert.doesNotMatch(source('AgentsSection.jsx'), /any harness installed|>Harnesses/)
  assert.equal(HARNESS_STOP_NAME, "Stop one app's chats")
  assert.equal(ACTIONS.find((a) => a.id === 'pick-harness').label, 'App (new chat)')
})

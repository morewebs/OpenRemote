// Every row the settings search can produce must be a row the view can point at.
//
// The search answers with a key, the view highlights `[data-stg="<key>"]`, and a
// key with no attribute behind it opens the right tab and marks nothing, which
// reads as a search that found something it cannot show. Two such keys shipped
// that way (an endpoint's add control, and the archived rows' Restore and
// Delete), so this pins the pair rather than each attribute by hand.
//
// Source text, because the console suite has no renderer. The keys the search
// builds interpolate an id (`endpoint-${h.id}-${entry.id}`), so both sides are
// compared with every `${...}` collapsed to one placeholder: what this proves is
// that a marking site exists for each shape of key, not that it lands on the row
// the person meant.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const read = (name) => readFileSync(new URL(`./${name}`, import.meta.url), 'utf8')

// The files that can carry a marking attribute: a key produced for one of these
// views has to be marked somewhere among them.
const VIEWS = ['SettingsView.jsx', 'McpServersSection.jsx', 'EndpointManager.jsx', 'PluginsView.jsx', 'PluginSettings.jsx', 'UpdatesRow.jsx']

const shape = (key) => key.replace(/\$\{[^}]+\}/g, '*')

test('every key the settings search can produce has a data-stg target', () => {
  const search = read('settings-search.js')
  const keys = new Set()
  for (const m of search.matchAll(/key:\s*(?:'([^']+)'|`([^`]+)`)/g)) keys.add(m[1] ?? m[2])
  assert.ok(keys.size >= 8, `the search produces its keys here (found ${keys.size})`)

  const marked = new Set()
  for (const file of VIEWS) {
    const text = read(file)
    for (const m of text.matchAll(/data-stg=(?:"([^"]+)"|\{`([^`]+)`\}|'([^']+)')/g)) {
      marked.add(m[1] ?? m[2] ?? m[3])
    }
  }
  const markedShapes = new Set([...marked].map(shape))
  const orphans = [...keys].filter((k) => !markedShapes.has(shape(k)))
  assert.deepEqual(orphans, [], `these search keys point at nothing: ${orphans.join(', ')}`)
})

test('the key the view highlights is the key the search answered with', () => {
  // The view looks the answer up by attribute, so the selector has to be the
  // plain key: a prefixed or case-folded selector would miss every dynamic key.
  const view = read('SettingsView.jsx')
  assert.match(view, /querySelector\(`\[data-stg="\$\{flash\}"\]`\)/)
})

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  SECTIONS,
  SETTING_GROUPS,
  SETTINGS_ENTRIES,
  dynamicEntries,
  hostOf,
  resultToOpen,
  searchSettings,
  visibleSections,
} from './settings-search.js'

test('section ids are unique and every entry points at one', () => {
  const ids = new Set(SECTIONS.map((s) => s.id))
  assert.equal(ids.size, SECTIONS.length)
  for (const entry of SETTINGS_ENTRIES) {
    assert.ok(ids.has(entry.section), `${entry.key} points at ${entry.section}`)
  }
  const keys = new Set(SETTINGS_ENTRIES.map((e) => e.key))
  assert.equal(keys.size, SETTINGS_ENTRIES.length, 'entry keys are unique')
})

test('every section sits in a declared group and no group is empty', () => {
  const groupIds = new Set(SETTING_GROUPS.map((g) => g.id))
  for (const section of SECTIONS) {
    assert.ok(groupIds.has(section.group), `${section.id} names ${section.group}`)
  }
  for (const group of SETTING_GROUPS) {
    assert.ok(
      SECTIONS.some((section) => section.group === group.id),
      `${group.label} owns a section`,
    )
  }
})

test('the daemon lives inside This install, not in a tab of its own', () => {
  assert.equal(
    SECTIONS.some((section) => section.id === 'daemon'),
    false,
    'there is no Daemon tab to keep in step',
  )
  for (const key of [
    'daemon-connection',
    'daemon-connect',
    'daemon-chats',
    'daemon-address',
    'daemon-restart',
  ]) {
    const entry = SETTINGS_ENTRIES.find((e) => e.key === key)
    assert.ok(entry, `${key} is indexed`)
    assert.equal(entry.section, 'this-install', `${key} points into This install`)
  }
})

test('an empty or whitespace query matches nothing', () => {
  assert.deepEqual(searchSettings(''), [])
  assert.deepEqual(searchSettings('   '), [])
})

test('an exact row label outranks a mention in another row', () => {
  const hits = searchSettings('reduce motion')
  assert.equal(hits[0].key, 'appearance-motion')
})

test('a synonym finds the row that means it', () => {
  const hotkeys = searchSettings('hotkey')
  assert.ok(hotkeys.length > 0)
  assert.equal(hotkeys[0].section, 'keyboard')
  assert.equal(searchSettings('blur')[0].key, 'appearance-privacy')
})

test('a section name opens that section first', () => {
  assert.equal(searchSettings('archived')[0].key, 'archived')
  // The daemon is not a tab any more: its rows live in This install, so the
  // word lands on the connection row there rather than on a tab that is gone.
  const daemon = searchSettings('daemon')
  assert.ok(daemon.length > 0, 'the word still finds something')
  assert.equal(daemon[0].section, 'this-install', 'and it sits in This install')
  assert.ok(
    daemon.some((entry) => entry.section === 'this-install' && entry.key === 'this-install'),
    'the tab itself answers "daemon" too',
  )
})

test('nonsense matches nothing rather than guessing', () => {
  assert.deepEqual(searchSettings('zzzxq'), [])
})

test('a phone sees the phone tabs, a desktop sees them all', () => {
  assert.deepEqual(
    visibleSections(true).map((s) => s.id),
    ['appearance', 'cloud', 'archived', 'this-install'],
  )
  assert.deepEqual(visibleSections(false), SECTIONS)
})

// ---- the rows whose list is daemon data ----

const HARNESSES = [
  { id: 'claude', available: true, custom_endpoint_supported: true },
  { id: 'codex', available: true, custom_endpoint_supported: false },
  { id: 'grok', available: false, custom_endpoint_supported: false },
]
const MCP = [{ id: 'p1', name: 'files' }]
const ARCHIVED = [{ id: 's1', harness: 'claude' }]
const CHATS = { s1: { title: 'Nightly review' } }

const withData = (over = {}) => [
  ...SETTINGS_ENTRIES,
  ...dynamicEntries({ harnesses: HARNESSES, mcpServers: MCP, archived: ARCHIVED, chats: CHATS, ...over }),
]

test('the dynamic rows are built from the lists the view draws, not a name list', () => {
  const base = withData()
  // A seventh harness is searchable the moment it exists, with no edit here.
  const seventh = [...HARNESSES, { id: 'agy', available: true, custom_endpoint_supported: false }]
  const grown = withData({ harnesses: seventh })
  assert.equal(grown.length, base.length + 1)
  assert.equal(searchSettings('agy', grown)[0]?.key, 'harness-agy')
  assert.equal(
    searchSettings('agy', base).some((e) => e.key === 'harness-agy'),
    false,
    'and it is not there before it exists',
  )
  // One row per archived chat, so a chat's own title finds it.
  assert.equal(searchSettings('nightly', base)[0]?.key, 'archived-s1')
  // The archived actions exist only while something is archived. The pin is
  // the archived section's own rows: another section's prose can fuzzy-match
  // the same letters, and that is not an archived row answering.
  const archivedRowsFor = (q, over) =>
    searchSettings(q, withData(over)).filter((e) => e.section === 'archived')
  assert.ok(archivedRowsFor('restore an archived chat', {}).length > 0)
  assert.deepEqual(archivedRowsFor('restore an archived chat', { archived: [] }), [])
  // No data, no dynamic rows at all.
  assert.deepEqual(dynamicEntries({}), [])
})

test('the words a person types find the control, in the right order', () => {
  const index = withData()
  // A harness, by its name and by the verb a newcomer uses.
  assert.equal(searchSettings('claude', index)[0].key, 'harness-claude')
  assert.equal(searchSettings('install claude', index)[0].key, 'harness-claude')
  assert.equal(searchSettings('install codex', index)[0].key, 'harness-codex')
  // Destructive actions before a fuzzy detail line.
  assert.equal(searchSettings('delete', index)[0].key, 'archived-delete-action')
  assert.equal(searchSettings('unarchive', index)[0].key, 'archived-restore-action')
  // The projects row answers the verb as well as the noun.
  assert.equal(searchSettings('remove project', index)[0].key, 'new-tasks-projects')
  assert.equal(searchSettings('unregister', index)[0].key, 'new-tasks-projects')
  // The keyboard bar's one button.
  assert.equal(searchSettings('reset', index)[0].key, 'keyboard-reset')
  assert.equal(searchSettings('reset every shortcut', index)[0].key, 'keyboard-reset')
  // The endpoint key control, and only for a harness that has endpoints.
  assert.equal(searchSettings('api key', index)[0].key, 'models-key-claude')
  assert.equal(
    searchSettings('api key', withData({ harnesses: [] })).some((e) => e.section === 'models'),
    false,
    'no endpoint key row without a harness that has endpoints',
  )
})

test('every query that should find nothing still finds nothing', () => {
  const index = withData()
  assert.deepEqual(searchSettings('zzzxq', index), [])
  assert.deepEqual(searchSettings('zzz qqq', index), [])
  // A word no row carries, even split into words that exist: "key" alone matches,
  // but "key zzzxq" must not, because every word has to match the same row.
  assert.equal(searchSettings('key zzzxq', index).length, 0)
  // Restored is not archived: no archived row answers when the list is empty.
  assert.equal(
    searchSettings('nightly', withData({ archived: [] })).filter((e) => e.section === 'archived').length,
    0,
  )
})

test('no row is indexed twice, with or without data', () => {
  for (const index of [withData(), SETTINGS_ENTRIES]) {
    const keys = index.map((e) => e.key)
    assert.equal(new Set(keys).size, keys.length, 'entry keys are unique')
  }
  const sections = new Set(SECTIONS.map((s) => s.id))
  for (const entry of withData()) {
    assert.ok(sections.has(entry.section), `${entry.key} points at ${entry.section}`)
  }
})
// ---- endpoints by the name their person gave them ----

const ENDPOINTS = {
  claude: [
    { id: 'e1', name: 'My gateway', base_url: 'https://gateway.internal:8080/v1', enabled: true },
    { id: 'e2', name: 'Home relay', base_url: 'http://192.168.1.9:4000', enabled: false },
  ],
}

test('a named endpoint is findable by its name and by its host', () => {
  const index = withData({ endpoints: ENDPOINTS })
  assert.equal(searchSettings('my gateway', index)[0].key, 'endpoint-claude-e1')
  assert.equal(searchSettings('gateway.internal', index)[0].key, 'endpoint-claude-e1', 'by its host')
  assert.equal(searchSettings('192.168.1.9', index)[0].key, 'endpoint-claude-e2', 'a bare address too')
  // The key row is still the one a key query lands on.
  assert.equal(searchSettings('api key', index)[0].key, 'models-key-claude')
  // And the endpoint row is not the key row.
  assert.notEqual(searchSettings('my gateway', index)[0].key, 'models-key-claude')
})

test('a renamed endpoint is findable by the new name and not the old', () => {
  const renamed = {
    claude: [{ id: 'e1', name: 'Work gateway', base_url: 'https://gateway.internal:8080/v1', enabled: true }],
  }
  const index = withData({ endpoints: renamed })
  assert.equal(searchSettings('work gateway', index)[0].key, 'endpoint-claude-e1')
  assert.equal(
    searchSettings('my gateway', index).some((e) => e.key === 'endpoint-claude-e1'),
    false,
    'the old name is gone with the rename',
  )
})

test('with no endpoints only the block and the key row are indexed', () => {
  for (const endpoints of [undefined, {}, { claude: [] }]) {
    const index = withData({ endpoints })
    const modelRows = index.filter((e) => e.section === 'models')
    assert.ok(modelRows.some((e) => e.key === 'models-claude'), 'the block stays')
    assert.ok(modelRows.some((e) => e.key === 'models-key-claude'), 'the key row stays')
    assert.equal(modelRows.filter((e) => e.key.startsWith('endpoint-')).length, 0, 'and nothing else')
  }
})

test('a host is read out of a base URL without trusting its shape', () => {
  assert.equal(hostOf('https://gateway.internal:8080/v1'), 'gateway.internal:8080')
  assert.equal(hostOf('gateway.internal/v1'), 'gateway.internal', 'no scheme still reads')
  assert.equal(hostOf('  '), '')
  assert.equal(hostOf('not a url at all'), 'not a url at all'.split('/')[0])
  assert.equal(hostOf(null), '')
})

test('Enter opens the first row, and nothing at all when there is no row', () => {
  // The search field's Enter goes through this rule, not its own copy of
  // "the first one": the rows and the keystroke must not be able to disagree.
  const rows = [
    { key: 'startup-reopen', label: 'Reopen', section: 'startup' },
    { key: 'appearance-motion', label: 'Motion', section: 'appearance' },
  ]
  assert.equal(resultToOpen(rows), rows[0])
  assert.equal(resultToOpen(rows, null), rows[0], 'no highlight is the first row')
  assert.equal(resultToOpen([rows[0]]), rows[0], 'one row is that row')
  // A highlight, when a list grows one, wins - and a highlight naming a row
  // that is no longer shown (the query changed) falls back to the first.
  assert.equal(resultToOpen(rows, 'appearance-motion'), rows[1])
  assert.equal(resultToOpen(rows, 'gone-row'), rows[0])
  // Nothing to open: Enter must do nothing, not act on the last answer.
  assert.equal(resultToOpen([]), null)
  assert.equal(resultToOpen(undefined), null)
  assert.equal(resultToOpen(null), null)
  assert.equal(resultToOpen('not a list'), null)
})

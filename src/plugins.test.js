import assert from 'node:assert/strict'
import test from 'node:test'
import {
  POWER_LABEL,
  RUNTIME_LABEL,
  STALE_MS,
  categories,
  clashes,
  entrySource,
  entryState,
  fetchedLabel,
  filterEntries,
  groupEntriesByCategory,
  installFailedNote,
  isStale,
  notSandboxed,
  pluginInitials,
  resolvePluginBrand,
  searchEntries,
  sourceLabel,
  sourceRequest,
  INSTALLED_EMPTY_NONE,
  INSTALLED_SEARCH_HINT_FROM,
  installedCountLine,
  installedEmptyLine,
  installedFields,
  installedSearchHint,
  matchesInstalled,
  searchInstalled,
} from './plugins.js'

test('a plugin that runs programs itself is labelled not sandboxed', () => {
  assert.equal(notSandboxed({ runtime: 'process', powers: ['run_programs'] }), true)
  assert.equal(notSandboxed({ runtime: 'content', powers: ['run_programs'] }), true)
  // A Claude plugin of skills alone runs nothing.
  assert.equal(notSandboxed({ runtime: 'content', powers: [] }), false)
  // JS and WebAssembly are sandboxed, whatever they ask for.
  assert.equal(notSandboxed({ runtime: 'js', powers: ['run_programs', 'microphone'] }), false)
  assert.equal(notSandboxed({ runtime: 'wasm', powers: ['run_programs'] }), false)
})

test('every power and runtime the daemon names has words', () => {
  for (const p of ['run_programs', 'read_chats', 'agent_tools', 'network', 'microphone']) {
    assert.ok(POWER_LABEL[p], p)
  }
  for (const r of ['process', 'js', 'wasm', 'content']) assert.ok(RUNTIME_LABEL[r], r)
})

test('the source reads back in a line, and the dialog asks for nothing empty', () => {
  assert.equal(sourceLabel({ kind: 'git', url: 'https://x/y' }), 'Git: https://x/y')
  assert.equal(sourceLabel({ kind: 'folder', path: 'C:\\p' }), 'Folder: C:\\p')
  assert.equal(
    sourceLabel({ kind: 'marketplace', marketplace: 'team', plugin: 'fmt', from: 'github.com/a/fmt' }),
    'team: github.com/a/fmt',
  )
  assert.deepEqual(sourceRequest('archive', ' https://x/a.zip '), { kind: 'archive', location: 'https://x/a.zip' })
  assert.equal(sourceRequest('git', '   '), null)
})

test('two plugins of one name both on are a clash; one turned off is not', () => {
  const installed = [
    { id: 'toy@local', name: 'toy', enabled: true },
    { id: 'toy@team', name: 'toy', enabled: true },
    { id: 'fmt@team', name: 'fmt', enabled: true },
    { id: 'fmt@local', name: 'fmt', enabled: false },
  ]
  assert.deepEqual(clashes(installed), [{ name: 'toy', ids: ['toy@local', 'toy@team'] }])
  assert.deepEqual(clashes([]), [])
})

test('a catalog is fetched again only when it is over a day old, and only if on', () => {
  const now = 10 * STALE_MS
  assert.equal(isStale({ enabled: true, refreshed_at: null }, now), true)
  assert.equal(isStale({ enabled: true, refreshed_at: now - STALE_MS - 1 }, now), true)
  assert.equal(isStale({ enabled: true, refreshed_at: now - 1000 }, now), false)
  assert.equal(isStale({ enabled: false, refreshed_at: null }, now), false)
  assert.equal(fetchedLabel({ refreshed_at: null }, now), 'Not fetched yet')
  assert.equal(fetchedLabel({ refreshed_at: now - 5 * 60_000 }, now), 'Fetched 5 min ago')
  assert.equal(fetchedLabel({ refreshed_at: now - 3 * 3_600_000 }, now), 'Fetched 3 h ago')
})

test('Discover filters by words, category and marketplace', () => {
  const entries = [
    { name: 'linter', description: 'Lints code', category: 'development', marketplace: 'a', tags: ['ci'] },
    { name: 'notes', description: 'Keeps notes', category: 'productivity', marketplace: 'b', author: 'Ann' },
  ]
  assert.deepEqual(filterEntries(entries, { query: 'lint' }).map((e) => e.name), ['linter'])
  assert.deepEqual(filterEntries(entries, { query: 'ci code' }).map((e) => e.name), ['linter'])
  assert.deepEqual(filterEntries(entries, { query: 'ann' }).map((e) => e.name), ['notes'])
  assert.deepEqual(filterEntries(entries, { category: 'productivity' }).map((e) => e.name), ['notes'])
  assert.deepEqual(filterEntries(entries, { marketplace: 'a' }).map((e) => e.name), ['linter'])
  assert.equal(filterEntries(entries).length, 2)
  assert.deepEqual(categories(entries), ['development', 'productivity'])
})

test('an entry installs by its marketplace and name, a command only once approved', () => {
  const entry = { name: 'gen', marketplace: 'team' }
  assert.deepEqual(entrySource(entry), { kind: 'marketplace', marketplace: 'team', plugin: 'gen' })
  assert.deepEqual(entrySource(entry, 'tool path'), {
    kind: 'marketplace',
    marketplace: 'team',
    plugin: 'gen',
    approve_command: 'tool path',
  })
  assert.equal(entryState({ installable: false }), 'unavailable')
  assert.equal(entryState({ installable: true, installed: null }), 'available')
  assert.equal(entryState({ installable: true, installed: {}, update: false }), 'installed')
  assert.equal(entryState({ installable: true, installed: {}, update: true }), 'update')
})

test('pluginInitials derives 1-2 uppercase characters or PL fallback', () => {
  assert.equal(pluginInitials('42crunch-api'), '42')
  assert.equal(pluginInitials('agent-sdk'), 'AS')
  assert.equal(pluginInitials('docker'), 'DO')
  assert.equal(pluginInitials(''), 'PL')
  assert.equal(pluginInitials('!!!'), 'PL')
  assert.equal(pluginInitials('a'), 'A')
  assert.equal(pluginInitials(null), 'PL')
})

test('groupEntriesByCategory clusters entries into title-cased categories and General fallback', () => {
  const entries = [
    { name: 'scanner', category: 'security' },
    { name: 'linter', category: 'development' },
    { name: 'debugger', category: 'development' },
    { name: 'misc-tool' },
    { name: 'empty-cat', category: '   ' },
  ]
  const groups = groupEntriesByCategory(entries)
  assert.deepEqual(
    groups.map((g) => ({ category: g.category, label: g.label })),
    [
      { category: 'development', label: 'Development' },
      { category: 'security', label: 'Security' },
      { category: 'general', label: 'General' },
    ],
  )
  assert.deepEqual(groups[0].entries.map((e) => e.name), ['linter', 'debugger'])
  assert.deepEqual(groups[1].entries.map((e) => e.name), ['scanner'])
  assert.deepEqual(groups[2].entries.map((e) => e.name), ['misc-tool', 'empty-cat'])

  assert.deepEqual(groupEntriesByCategory([]), [])
  assert.deepEqual(groupEntriesByCategory(null), [])
})

test('searchEntries ranks exact and prefix matches ahead of description-only matches', () => {
  const entries = [
    { name: 'code-helper', description: 'Includes a lint feature' },
    { name: 'linter', description: 'Checks JavaScript syntax' },
    { name: 'lint', description: 'Exact match tool' },
    { name: 'unrelated', description: 'Does other tasks' },
  ]
  const results = searchEntries(entries, { query: 'lint' })
  assert.deepEqual(results.map((e) => e.name), ['lint', 'linter', 'code-helper'])
})

test('searchEntries handles regex characters safely without throwing', () => {
  const entries = [
    { name: 'regex-special.*[tool]', description: 'Has special.*[regex] characters' },
    { name: 'normal-tool', description: 'Standard description' },
  ]
  assert.doesNotThrow(() => {
    const results = searchEntries(entries, { query: 'special.*[regex]' })
    assert.equal(results.length, 1)
    assert.equal(results[0].name, 'regex-special.*[tool]')
  })
  assert.doesNotThrow(() => {
    const emptyResults = searchEntries(entries, { query: 'notfound.*[pattern]' })
    assert.equal(emptyResults.length, 0)
  })
})

test('searchEntries filters by category and marketplace and sorts alphabetically when query is absent', () => {
  const entries = [
    { name: 'zebra', category: 'tools', marketplace: 'core' },
    { name: 'alpha', category: 'tools', marketplace: 'core' },
    { name: 'beta', category: 'other', marketplace: 'community' },
  ]
  const inTools = searchEntries(entries, { category: 'tools' })
  assert.deepEqual(inTools.map((e) => e.name), ['alpha', 'zebra'])

  const inCommunity = searchEntries(entries, { marketplace: 'community' })
  assert.deepEqual(inCommunity.map((e) => e.name), ['beta'])
})

test('resolvePluginBrand identifies recognized brands from id and name or returns null', () => {
  assert.equal(resolvePluginBrand('gh-cli', 'github-tool'), 'github')
  assert.equal(resolvePluginBrand('org.gitlab.runner', 'GitLab CI'), 'gitlab')
  assert.equal(resolvePluginBrand('docker-compose', 'Docker Compose'), 'docker')
  assert.equal(resolvePluginBrand('android-tools', 'Android Studio Bridge'), 'android')
  assert.equal(resolvePluginBrand('aws-lambda', 'AWS Cloud Tools'), 'aws')
  assert.equal(resolvePluginBrand('slack-notifications', 'Slack Bot'), 'slack')
  assert.equal(resolvePluginBrand('pdf-reader', 'PDF Export'), 'pdf')
  assert.equal(resolvePluginBrand('jupyter-lab', 'Jupyter Notebooks'), 'jupyter')
  assert.equal(resolvePluginBrand('python-runner', 'Python Linter'), 'python')
  assert.equal(resolvePluginBrand('figma-sync', 'Figma Exporter'), 'figma')
  assert.equal(resolvePluginBrand('freebuff', 'Freebuff'), 'freebuff')
  assert.equal(resolvePluginBrand('morewebs/freebuff', 'freebuff-tool'), 'freebuff')

  // Case insensitive and partial / token matching
  assert.equal(resolvePluginBrand('GITHUB', 'tool'), 'github')
  assert.equal(resolvePluginBrand(null, 'Slack'), 'slack')
  assert.equal(resolvePluginBrand('docker', null), 'docker')

  // Unrecognized tools return null
  assert.equal(resolvePluginBrand('custom-plugin', 'My Custom Plugin'), null)
  assert.equal(resolvePluginBrand('', ''), null)
  assert.equal(resolvePluginBrand(null, null), null)
})

test('searchEntries and groupEntriesByCategory handle null entries, null filters, and malformed tags defensively', () => {
  const malformedEntries = [
    null,
    undefined,
    { name: 'alpha', category: 'tools', tags: 'not-an-array' },
    { name: 'beta', category: 'tools', tags: null },
    { name: 'gamma', tags: ['good'] },
  ]

  // searchEntries with null filters
  assert.doesNotThrow(() => {
    const res = searchEntries(malformedEntries, null)
    assert.equal(res.length, 3)
    assert.deepEqual(res.map((e) => e.name), ['alpha', 'beta', 'gamma'])
  })

  // searchEntries query on malformed tags and null entries
  assert.doesNotThrow(() => {
    const res = searchEntries(malformedEntries, { query: 'good' })
    assert.equal(res.length, 1)
    assert.equal(res[0].name, 'gamma')
  })

  // groupEntriesByCategory with null/undefined elements
  assert.doesNotThrow(() => {
    const groups = groupEntriesByCategory(malformedEntries)
    assert.equal(groups.length, 2)
  })
})

test('pluginInitials handles edge cases including whitespace, symbols, numbers, and multi-segment names', () => {
  // Empty string and whitespace
  assert.equal(pluginInitials(''), 'PL')
  assert.equal(pluginInitials('   '), 'PL')
  assert.equal(pluginInitials('\t\n  '), 'PL')

  // Symbols-only strings
  assert.equal(pluginInitials('!@#$%'), 'PL')
  assert.equal(pluginInitials('---_+++'), 'PL')
  assert.equal(pluginInitials('   ***   '), 'PL')

  // Numbers-only
  assert.equal(pluginInitials('1234'), '12')
  assert.equal(pluginInitials('7'), '7')

  // Single-char
  assert.equal(pluginInitials('x'), 'X')
  assert.equal(pluginInitials('Z'), 'Z')

  // Multi-segment
  assert.equal(pluginInitials('alpha-beta-gamma'), 'AB')
  assert.equal(pluginInitials('one_two_three_four'), 'OT')
})

test('searchEntries handles regex special characters safely in queries', () => {
  const entries = [
    {
      name: 'regex-engine',
      display_name: 'foo(bar)?',
      description: 'Supports [a-z]+ and \\d+ patterns with ^start and *test*',
    },
    {
      name: 'plain-engine',
      display_name: 'Plain Engine',
      description: 'Standard text tool',
    },
  ]

  const regexQueries = ['[a-z]+', 'foo(bar)?', '*test*', '\\d+', '^start']
  for (const q of regexQueries) {
    assert.doesNotThrow(() => {
      const results = searchEntries(entries, { query: q })
      assert.equal(results.length, 1)
      assert.equal(results[0].name, 'regex-engine')
    })
  }

  // Non-matching regex strings safely return empty results
  const nonMatching = ['[0-9]+', 'missing(pattern)?', '*notfound*', '\\s+', '^other']
  for (const q of nonMatching) {
    assert.doesNotThrow(() => {
      const results = searchEntries(entries, { query: q })
      assert.equal(results.length, 0)
    })
  }
})

test('searchEntries handles multiple whitespace runs, tabs, and leading/trailing whitespace in query', () => {
  const entries = [
    { name: 'alpha-tool', description: 'Primary alpha tool' },
    { name: 'beta-tool', description: 'Beta utility' },
    { name: 'alpha-beta-combo', description: 'Both alpha and beta integrated' },
  ]

  // Query with tab characters, multiple spaces, leading/trailing whitespace
  const queryWithWhitespace = '  \t  alpha   \t  beta  '
  const results = searchEntries(entries, { query: queryWithWhitespace })
  assert.equal(results.length, 1)
  assert.equal(results[0].name, 'alpha-beta-combo')

  // Whitespace-only queries sort alphabetically without filtering
  const blankResults = searchEntries(entries, { query: '   \t  \n  ' })
  assert.equal(blankResults.length, 3)
  assert.deepEqual(
    blankResults.map((e) => e.name),
    ['alpha-beta-combo', 'alpha-tool', 'beta-tool'],
  )
})

test('searchEntries safely processes entries with missing fields', () => {
  const sparseEntries = [
    {
      name: 'sparse-plugin',
      description: null,
      author: undefined,
      tags: null,
      category: undefined,
    },
    {
      name: 'complete-plugin',
      display_name: 'Complete Plugin',
      description: 'Fully defined metadata',
      author: 'Tester',
      tags: ['tool', 'utility'],
      category: 'developer',
    },
  ]

  // Search by name of sparse entry
  const resName = searchEntries(sparseEntries, { query: 'sparse' })
  assert.equal(resName.length, 1)
  assert.equal(resName[0].name, 'sparse-plugin')

  // Search by query that matches nothing in sparse entry
  const resMiss = searchEntries(sparseEntries, { query: 'nonexistent' })
  assert.equal(resMiss.length, 0)

  // Filtering by category general includes undefined category
  const resGeneral = searchEntries(sparseEntries, { category: 'general' })
  assert.equal(resGeneral.length, 1)
  assert.equal(resGeneral[0].name, 'sparse-plugin')

  // Filtering by developer category excludes undefined category
  const resDev = searchEntries(sparseEntries, { category: 'developer' })
  assert.equal(resDev.length, 1)
  assert.equal(resDev[0].name, 'complete-plugin')
})

test('groupEntriesByCategory handles groups of 6, missing categories, and empty inputs', () => {
  // Category with exactly 6 plugins
  const sixEntries = Array.from({ length: 6 }, (_, i) => ({
    name: `tool-${i + 1}`,
    category: 'utilities',
  }))
  const sixGroups = groupEntriesByCategory(sixEntries)
  assert.equal(sixGroups.length, 1)
  assert.equal(sixGroups[0].category, 'utilities')
  assert.equal(sixGroups[0].label, 'Utilities')
  assert.equal(sixGroups[0].entries.length, 6)
  assert.deepEqual(
    sixGroups[0].entries.map((e) => e.name),
    ['tool-1', 'tool-2', 'tool-3', 'tool-4', 'tool-5', 'tool-6'],
  )

  // Entries with missing category land in General
  const missingCatEntries = [
    { name: 'no-cat-1', category: undefined },
    { name: 'no-cat-2', category: null },
    { name: 'no-cat-3', category: '' },
    { name: 'no-cat-4', category: '   ' },
    { name: 'no-cat-5' },
    { name: 'named-cat', category: 'monitoring' },
  ]
  const catGroups = groupEntriesByCategory(missingCatEntries)
  assert.equal(catGroups.length, 2)
  assert.equal(catGroups[0].category, 'monitoring')
  assert.equal(catGroups[0].label, 'Monitoring')
  assert.equal(catGroups[0].entries.length, 1)

  assert.equal(catGroups[1].category, 'general')
  assert.equal(catGroups[1].label, 'General')
  assert.equal(catGroups[1].entries.length, 5)
  assert.deepEqual(
    catGroups[1].entries.map((e) => e.name),
    ['no-cat-1', 'no-cat-2', 'no-cat-3', 'no-cat-4', 'no-cat-5'],
  )

  // Empty entries array returns empty array
  assert.deepEqual(groupEntriesByCategory([]), [])
  assert.deepEqual(groupEntriesByCategory(null), [])
  assert.deepEqual(groupEntriesByCategory(undefined), [])
})

test('category filter options and searchEntries handle General and case-insensitive categories', () => {
  const mixedEntries = [
    { name: 'alpha', category: 'developer-tools' },
    { name: 'beta', category: 'SECURITY' },
    { name: 'gamma', category: null },
    { name: 'delta', category: 'General' },
    { name: 'epsilon' },
  ]

  // Verify groupEntriesByCategory derives title-cased labels with General last
  const groups = groupEntriesByCategory(mixedEntries)
  const options = groups.map((g) => ({ value: g.category, label: g.label }))
  assert.deepEqual(options, [
    { value: 'developer-tools', label: 'Developer Tools' },
    { value: 'security', label: 'Security' },
    { value: 'general', label: 'General' },
  ])

  // searchEntries by category 'general' captures explicit General and uncategorized
  const generalResults = searchEntries(mixedEntries, { category: 'general' })
  assert.deepEqual(
    generalResults.map((e) => e.name),
    ['delta', 'epsilon', 'gamma'],
  )

  // searchEntries by category 'General' (title-case query) works identically
  const generalTitleResults = searchEntries(mixedEntries, { category: 'General' })
  assert.deepEqual(
    generalTitleResults.map((e) => e.name),
    ['delta', 'epsilon', 'gamma'],
  )

  // searchEntries by category 'security' matches case-insensitively
  const secResults = searchEntries(mixedEntries, { category: 'security' })
  assert.deepEqual(
    secResults.map((e) => e.name),
    ['beta'],
  )
})

test('a failed install sends the user back to reading the source', () => {
  // The daemon's own words come through, and the sentence adds what the
  // screen now does: the review it came from is spent, so the way back is
  // the source, not an Install to press again.
  const said = installFailedNote('the folder is in use by another program')
  assert.match(said, /the folder is in use by another program/)
  assert.match(said, /Nothing was installed/)
  assert.match(said, /open the plugin again from where you found it/)
  // A failure the daemon described with nothing at all still says
  // something a person can act on.
  const bare = installFailedNote('')
  assert.match(bare, /The install did not go through/)
  assert.match(bare, /open the plugin again from where you found it/)
  assert.doesNotMatch(bare, /^ .*Nothing was installed/)
  // It never claims the install can be retried from the same review.
  assert.doesNotMatch(said, /try again/i)
  assert.doesNotMatch(said, /press install/i)
})

// ---- the Installed tab's search ----

// An installed plugin as the daemon lists it: the name, the runtime, the
// source it came from, and the powers it was given.
const pluginOf = (over = {}) => ({
  id: 'p1',
  name: 'Docker tools',
  runtime: 'content',
  powers: [],
  source: { kind: 'folder', path: 'C:\\work\\docker-tools' },
  ...over,
})

test('the search reads only the fields an installed row prints', () => {
  const plugin = pluginOf()
  // The name, the runtime chip's words, the source line: three fields.
  assert.deepEqual(installedFields(plugin), [
    'docker tools',
    'claude plugin',
    'folder: c:\\work\\docker-tools',
  ])
  for (const query of ['docker', 'claude plugin', 'docker-tools']) {
    assert.equal(matchesInstalled(plugin, query), true, query)
  }
})

test('a word cannot be assembled from the end of one field and the start of another', () => {
  // The name ends in "docker" and the runtime begins with "claude plugin": a
  // joined string would contain "dockerclaude" and match it, while the row
  // shows those words apart. Per-field matching must refuse it.
  const plugin = pluginOf({ name: 'Docker', runtime: 'content' })
  assert.equal(matchesInstalled(plugin, 'dockerclaude'), false)
  // Two real words are still fine, because each is found inside one field.
  assert.equal(matchesInstalled(plugin, 'docker claude'), true)
})

test('nothing hidden is searched: no id, no powers, no version', () => {
  const plugin = pluginOf({ id: 'secret-id', version: '9.9.9', powers: ['network'] })
  for (const query of ['secret-id', 'network', '9.9.9']) {
    assert.equal(matchesInstalled(plugin, query), false, query)
  }
})

test('each source kind is searchable by what its line says', () => {
  const git = pluginOf({ source: { kind: 'git', url: 'https://example.test/tools.git' } })
  assert.equal(matchesInstalled(git, 'example.test'), true)
  const archive = pluginOf({ source: { kind: 'archive', location: 'C:\\downloads\\tools.zip' } })
  assert.equal(matchesInstalled(archive, 'tools.zip'), true)
  const market = pluginOf({ source: { kind: 'marketplace', marketplace: 'moreweb', from: 'tools@1.2.0' } })
  assert.equal(matchesInstalled(market, 'moreweb'), true)
  assert.equal(matchesInstalled(market, 'tools@1.2.0'), true)
})

test('case and whitespace do not matter, and an empty query filters nothing', () => {
  const plugin = pluginOf()
  assert.equal(matchesInstalled(plugin, '  DOCKER  '), true)
  assert.equal(matchesInstalled(plugin, 'docker   tools'), true)
  assert.equal(matchesInstalled(plugin, 'docker podman'), false)
  for (const empty of ['', '   ', null, undefined]) {
    assert.equal(matchesInstalled(plugin, empty), true)
  }
})

test('the search keeps the list order and drops only non-matches', () => {
  const installed = [
    pluginOf({ id: 'a', name: 'Docker tools', source: { kind: 'folder', path: 'C:\\a' } }),
    pluginOf({ id: 'b', name: 'Notes', runtime: 'js', source: { kind: 'folder', path: 'C:\\b' } }),
    pluginOf({ id: 'c', name: 'Docker compose', source: { kind: 'folder', path: 'C:\\c' } }),
  ]
  assert.deepEqual(searchInstalled(installed, 'docker').map((p) => p.id), ['a', 'c'])
  assert.deepEqual(searchInstalled(installed, 'javascript').map((p) => p.id), ['b'])
  assert.deepEqual(searchInstalled(installed, '').map((p) => p.id), ['a', 'b', 'c'])
  assert.deepEqual(searchInstalled(null, 'docker'), [])
})

test('the count line and the two empty answers stay apart', () => {
  assert.equal(installedCountLine(3, 12), '3 of 12 plugins')
  assert.equal(installedCountLine(1, 1), '1 of 1 plugin')
  assert.equal(installedEmptyLine(''), INSTALLED_EMPTY_NONE)
  assert.equal(installedEmptyLine('   '), INSTALLED_EMPTY_NONE)
  assert.match(INSTALLED_EMPTY_NONE, /^Nothing installed yet\./)
  assert.equal(installedEmptyLine('  podman '), 'No plugins match “podman”.')
  assert.notEqual(installedEmptyLine('podman'), INSTALLED_EMPTY_NONE)
})

test('the hint appears when the list is long enough to need one', () => {
  for (const total of [0, 1, 4]) assert.equal(installedSearchHint(total), '')
  assert.equal(INSTALLED_SEARCH_HINT_FROM, 5)
  for (const total of [5, 9, 30]) assert.equal(installedSearchHint(total), 'Search installed plugins')
  assert.equal(installedSearchHint(undefined), '')
})



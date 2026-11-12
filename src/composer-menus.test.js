import assert from 'node:assert/strict'
import test from 'node:test'
import {
  agentInsertion,
  applyPick,
  argItems,
  chatInsertion,
  commandItems,
  fileInsertion,
  filesForQuery,
  holdDismissal,
  mentionItems,
  mentionedChats,
  menuAt,
  menuKeyOf,
  ownCommands,
  takeCommands,
} from './composer-menus.js'

test('the / and @ menus open at the start of a word, anywhere in the message', () => {
  assert.deepEqual(menuAt('/com', 4), { kind: '/', query: 'com', start: 0, end: 4 })
  assert.deepEqual(menuAt('fix it /rev', 11), { kind: '/', query: 'rev', start: 7, end: 11 })
  assert.deepEqual(menuAt('look at @src/ap', 15), { kind: '@', query: 'src/ap', start: 8, end: 15 })
  // a path is not a command, an email is not a mention
  assert.equal(menuAt('src/app.js', 10), null)
  assert.equal(menuAt('/usr/bin', 8), null)
  assert.equal(menuAt('me@example.com', 14), null)
  // after an OpenRemote command that takes a choice, the choices open
  assert.deepEqual(menuAt('/model ', 7), { kind: 'arg', command: 'model', arg: 'model', query: '', start: 7, end: 7 })
  assert.deepEqual(menuAt('/effort hi', 10), {
    kind: 'arg',
    command: 'effort',
    arg: 'effort',
    query: 'hi',
    start: 8,
    end: 10,
  })
  // a free-text argument has no menu
  assert.equal(menuAt('/rename ', 8), null)
})

test('a pick replaces the word and leaves the caret after it', () => {
  const menu = menuAt('please /com now', 11)
  assert.deepEqual(applyPick('please /com now', menu, '/compact'), { text: 'please /compact now', caret: 15 })
  const at = menuAt('@not', 4)
  assert.deepEqual(applyPick('@not', at, '@notes:', { space: false }), { text: '@notes:', caret: 7 })
})

test('what each pick inserts', () => {
  assert.equal(fileInsertion('src/app.js'), '@src/app.js')
  assert.equal(fileInsertion('my notes/a.md'), '@"my notes/a.md"')
  assert.equal(agentInsertion('Explore', 'claude'), '@agent-Explore')
  assert.equal(agentInsertion('notes:scribe', 'claude'), '@agent-notes:scribe')
  assert.equal(agentInsertion('general', 'opencode'), '@general')
  assert.equal(
    chatInsertion({ id: '3f2a9c1e-0000-4000-8000-000000000000', title: 'Fix the "login" bug' }),
    '@chat "Fix the login bug" (3f2a9c1e)',
  )
})

test('a message names the chats it points at by their ids', () => {
  const chats = [{ id: '3f2a9c1e-aaaa' }, { id: '9b8c7d6e-bbbb' }]
  const text = 'compare @chat "One" (3f2a9c1e) with @chat "Two" (9b8c7d6e) and @chat "Gone" (00000000)'
  assert.deepEqual(mentionedChats(text, chats), ['3f2a9c1e-aaaa', '9b8c7d6e-bbbb'])
  // the console's own shape: chats keyed by id
  const keyed = Object.fromEntries(chats.map((c) => [c.id, c]))
  assert.deepEqual(mentionedChats(text, keyed), ['3f2a9c1e-aaaa', '9b8c7d6e-bbbb'])
})

test("OpenRemote's commands run at send and leave the rest of the message", () => {
  assert.deepEqual(takeCommands('/model opus fix the bug'), {
    run: [{ name: 'model', arg: 'opus' }],
    text: 'fix the bug',
  })
  assert.deepEqual(takeCommands('fix it /effort high and /sync'), {
    run: [
      { name: 'effort', arg: 'high' },
      { name: 'sync', arg: null },
    ],
    text: 'fix it and',
  })
  assert.deepEqual(takeCommands('/rename "Login work" then go'), {
    run: [{ name: 'rename', arg: 'Login work' }],
    text: 'then go',
  })
  // /fast takes on/off only when it is there
  assert.deepEqual(takeCommands('/fast fix the bug'), { run: [{ name: 'fast', arg: null }], text: 'fix the bug' })
  assert.deepEqual(takeCommands('/fast off'), { run: [{ name: 'fast', arg: 'off' }], text: '' })
  // the harness's own commands stay as typed
  assert.deepEqual(takeCommands('/compact please'), { run: [], text: '/compact please' })
})

test('the / menu hides harness names OpenRemote takes over and dims what cannot run', () => {
  const own = ownCommands({ inChat: true, cloud: false })
  assert.ok(!own.some((c) => c.name === 'sync'), 'no Cloud, no /sync')
  assert.ok(!ownCommands({ inChat: false, cloud: true }).some((c) => c.name === 'stop'))
  const items = commandItems('', {
    harnessCommands: [
      { name: 'compact', description: 'Compact', source: 'harness', available: true },
      { name: 'model', description: "claude's own", source: 'harness', available: true },
      { name: 'notes:jot', description: 'Jot', source: 'plugin', available: false, why: 'needs a harness that runs plugins' },
    ],
    own,
  })
  const labels = items.map((i) => i.label)
  assert.ok(labels.includes('/compact'), 'an everyday command shows before typing')
  assert.equal(items.filter((i) => i.label === '/model').length, 1)
  assert.equal(items.find((i) => i.label === '/model').group, 'OpenRemote')
  assert.ok(!labels.includes('/notes:jot'), 'skills wait for a search')
  assert.ok(items.at(-1).note, 'and the menu says how much a search finds')
  const jot = commandItems('jot', {
    harnessCommands: [{ name: 'notes:jot', description: 'Jot', source: 'plugin', available: false, why: 'x' }],
  }).find((i) => i.label === '/notes:jot')
  assert.equal(jot.group, 'Skills')
  assert.equal(jot.disabled, true)
  assert.equal(jot.badge, 'plugin')
  // a query ranks names that start with it first
  assert.equal(commandItems('com', { harnessCommands: [{ name: 'compact' }, { name: 'recompile' }] })[0].label, '/compact')
})

test('the / menu leaves out what the harness keeps for itself and keeps rows short', () => {
  const harnessCommands = [
    { name: '__remote-workflow', description: 'Internal' },
    { name: 'agents', description: '(removed) Ask Claude to manage subagents' },
    { name: 'advisor', description: 'Let Claude consult a stronger model at key moments. More text here.', argument_hint: '[fable|opus|sonnet|off]' },
    { name: 'auto-mode-setup', description: 'Set up', argument_hint: '[--request-id <uuid>] (--wizard posture=...)' },
    { name: 'deep-research', description: 'Use this skill when the prompt needs research' },
  ]
  const labels = commandItems('a', { harnessCommands }).map((i) => i.label)
  assert.ok(!labels.includes('/__remote-workflow'))
  assert.ok(!labels.includes('/agents'))
  const advisor = commandItems('advisor', { harnessCommands })[0]
  assert.equal(advisor.detail, 'Let Claude consult a stronger model at key moments.')
  assert.equal(advisor.hint, '[fable|opus|sonnet|off]', 'a short hint stays')
  assert.equal(commandItems('auto', { harnessCommands })[0].hint, '', 'a usage line goes')
  assert.equal(commandItems('deep', { harnessCommands })[0].group, 'Skills')
})

test('an Esc closes the word it was pressed in, never the composer', () => {
  // The composer's menu as the hook runs it: the word under the caret, the
  // dismissal an Esc left behind, and what the helpers say to do with it on
  // every render.
  const step = (held, text, caret, esc = false) => {
    const menu = menuAt(text, caret)
    const key = menuKeyOf(menu)
    const dismissed = holdDismissal(menu, esc && menu ? key : held)
    return { dismissed, open: Boolean(menu) && dismissed !== key }
  }

  // "/" opens the menu.
  let state = step(null, '/', 1)
  assert.equal(state.open, true)
  // Esc closes it: the dismissal is that word, at that offset.
  state = step(state.dismissed, '/', 1, true)
  assert.equal(state.dismissed, '/:0')
  assert.equal(state.open, false)
  // The box is cleared: the caret has left the word, so nothing is dismissed.
  state = step(state.dismissed, '', 0)
  assert.equal(state.dismissed, null)
  // A fresh "/" at the same offset opens again, for this composer's life.
  state = step(state.dismissed, '/', 1)
  assert.equal(state.open, true)
  // Typing on inside the dismissed word keeps it shut: the offset never
  // moves, so the dismissal still covers the word it was pressed in.
  state = step(null, '/te', 3)
  assert.equal(state.open, true)
  state = step(state.dismissed, '/te', 3, true)
  assert.equal(state.open, false)
  state = step(state.dismissed, '/tes', 4)
  assert.equal(state.open, false, 'a longer word at the same offset is still that word')
  // A word in the middle of the message closes and reopens the same way.
  state = step(null, 'fix the /tests', 14)
  assert.equal(state.open, true)
  state = step(state.dismissed, 'fix the /tests', 14, true)
  assert.equal(state.open, false)
  state = step(state.dismissed, 'fix the', 7)
  assert.equal(state.dismissed, null)
  state = step(state.dismissed, 'fix the /tests', 14)
  assert.equal(state.open, true)
})

test('a list that did not load says so instead of showing another chat rows', () => {
  const line = 'Commands and agents did not load. They load again when you switch chats or the connection comes back.'
  const own = ownCommands({ inChat: true, cloud: false })
  const empty = commandItems('', { harnessCommands: [], own, unavailable: line })
  const last = empty[empty.length - 1]
  assert.equal(last.note, true)
  assert.equal(last.detail, line)
  assert.equal(last.disabled, true, 'a note is read, never picked')
  assert.equal(last.insertion, '')
  // While typing, and in the @ menu, the same line closes the list.
  assert.equal(commandItems('mod', { own, unavailable: line }).at(-1).detail, line)
  const at = mentionItems('', { chats: [{ id: 'abc', title: 'Notes' }], unavailable: line })
  assert.deepEqual(at.map((i) => i.group), ['Chats', undefined])
  assert.equal(at.at(-1).detail, line)
  // An ask that answered adds no line.
  assert.ok(!commandItems('', { own }).some((i) => i.detail === line))
})

test('a menu trimmed to its last row still ends with the line that says it did not load', () => {
  const line = 'Commands and agents did not load. They load again when you switch chats or the connection comes back.'
  // More rows than the menu takes: the chats group alone can be this long.
  const files = Array.from({ length: 90 }, (_, i) => ({ path: `src/f${i}.js`, kind: 'file' }))
  const trimmed = mentionItems('', { files, unavailable: line })
  assert.equal(trimmed.length, 80, 'the budget for rows does not grow for the lines')
  assert.equal(trimmed.at(-1).detail, line, 'the failure line survives the trim')
  assert.equal(trimmed.at(-1).note, true)
  assert.equal(trimmed.filter((i) => i.kind === 'file').length, 78, 'both lines cost a row each')
  assert.equal(trimmed.at(-2).detail, '12 more - keep typing to narrow')
  // An answer that arrived is trimmed to the full eighty: seventy-nine rows
  // and the line that says the rest are there.
  assert.equal(mentionItems('', { files }).length, 80)
})

test('the @ menu only shows the files asked for the word being typed', () => {
  const held = {
    q: 'not',
    entries: [
      { path: 'notes.md', kind: 'file' },
      { path: 'readme.md', kind: 'file' },
    ],
  }
  assert.deepEqual(filesForQuery(held, 'not'), held.entries, 'the word that asked gets its list')
  assert.deepEqual(filesForQuery(held, 'other'), [], 'another word does not inherit it')
  assert.deepEqual(filesForQuery(null, 'not'), [], 'nothing asked, nothing shown')
  // Typing on inside the same word keeps the rows that still match while the
  // next answer is on its way: emptying the menu on every keystroke made it
  // flicker, since it came back a beat later.
  assert.deepEqual(filesForQuery(held, 'noted').map((f) => f.path), ['notes.md'])
  assert.deepEqual(filesForQuery(held, 'zzz'), [], 'a word nothing matches has no rows')
  // A word whose ask failed holds an empty answer, not the previous one.
  assert.deepEqual(filesForQuery({ q: 'not', entries: [] }, 'not'), [])
})

test('the @ menu groups files, agents, servers and chats', () => {
  const items = mentionItems('no', {
    files: [{ path: 'notes.md', kind: 'file' }, { path: 'no-docs', kind: 'dir' }],
    agents: [{ name: 'notes:scribe', source: 'plugin', available: true }],
    servers: [{ name: 'notes-db', source: 'plugin', plugin: 'notes@local' }],
    chats: [{ id: 'abc12345-x', title: 'Notes cleanup', harness: 'claude' }],
    harness: 'claude',
  })
  assert.deepEqual(
    items.map((i) => i.group),
    ['Files', 'Files', 'Agents', 'MCP servers', 'Chats'],
  )
  assert.equal(items[1].label, 'no-docs/')
  assert.equal(items[2].insertion, '@agent-notes:scribe')
  assert.equal(items[3].insertion, '@notes-db:')
  assert.equal(items[3].space, false)
  assert.equal(argItems('op', [{ id: 'opus', name: 'Opus' }, { id: 'haiku' }]).map((i) => i.insertion)[0], 'opus')
})

// ---- the @ menu's files: narrowed by the word, and honest about a cap ----

test('typing narrows the files the ask returned, the way it narrows agents', () => {
  const files = [
    { path: 'src/composer-menus.js', kind: 'file' },
    { path: 'README.md', kind: 'file' },
    { path: 'src/split.js', kind: 'file' },
  ]
  assert.deepEqual(
    mentionItems('composer', { files }).map((i) => i.label),
    ['src/composer-menus.js'],
    'only the file that carries the word',
  )
  // A word nothing matches leaves no files at all, so the menu can say it is
  // empty rather than showing rows that cannot be what was meant.
  assert.deepEqual(mentionItems('zzz', { files }), [])
  // The path matcher is the one the file ask already uses: every character of
  // the word, in order, anywhere (the daemon ranks a path the same way).
  assert.deepEqual(
    mentionItems('cmn', { files }).map((i) => i.label),
    ['src/composer-menus.js'],
    'a subsequence of a path still finds it',
  )
  // An empty word is not a filter.
  assert.equal(mentionItems('', { files }).length, 3)
})

test('the cap keeps a line that says the list is short', () => {
  const files = Array.from({ length: 90 }, (_, i) => ({ path: `src/file-${i}.js`, kind: 'file' }))
  const trimmed = mentionItems('src', { files })
  assert.equal(trimmed.length, 80, 'the budget for rows does not grow for the line')
  assert.equal(trimmed.filter((i) => i.kind === 'file').length, 79, 'the line costs one row')
  const last = trimmed.at(-1)
  assert.equal(last.note, true, 'the last row is the line, not a file')
  assert.equal(last.detail, '11 more - keep typing to narrow')
  assert.equal(last.disabled, true, 'a note is read, never picked')
  // Both lines fit when an ask also failed: the trim line, then the failure.
  const both = mentionItems('src', { files, unavailable: 'Commands and agents did not load.' })
  assert.equal(both.length, 80)
  assert.equal(both.at(-2).detail, '12 more - keep typing to narrow')
  assert.equal(both.at(-1).detail, 'Commands and agents did not load.')
})

test('a failed file ask leaves no stale rows and says so once', () => {
  // What the hook holds after an ask fails for a newer word: nothing that
  // belongs to the word before it.
  const stale = filesForQuery({ q: 'src', entries: [{ path: 'src/a.js', kind: 'file' }] }, 'notes')
  assert.deepEqual(stale, [])
  const line = 'Files did not load. They are asked again as you type.'
  const items = mentionItems('notes', { files: stale, filesUnavailable: line })
  assert.deepEqual(
    items.map((i) => i.kind),
    ['note'],
    'the menu says the ask failed and shows no files',
  )
  assert.equal(items[0].detail, line)
  assert.equal(items[0].disabled, true)
  // The same word keeping its own failed answer is still the failure, not the
  // rows from a beat ago.
  assert.deepEqual(filesForQuery({ q: 'notes', entries: [] }, 'notes'), [])
})

test('an ask that answered nothing leaves an empty menu, or the line if it failed', () => {
  assert.deepEqual(mentionItems('notes', { files: [] }), [], 'nothing to show, nothing said')
  const line = 'Files did not load. They are asked again as you type.'
  assert.deepEqual(
    mentionItems('notes', { files: [], filesUnavailable: line }).map((i) => i.detail),
    [line],
  )
  // An answer is not the failure: no line when the ask worked.
  assert.deepEqual(
    mentionItems('notes', { files: [], filesUnavailable: null }),
    [],
    'a successful answer says nothing',
  )
})

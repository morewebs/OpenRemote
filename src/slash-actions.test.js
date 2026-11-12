import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  CONSOLE_ACTIONS,
  consoleCommands,
  filterHelp,
  helpGroups,
  lastAnswer,
  leadingCommand,
  pendingHint,
  recentKey,
  refusalFor,
  statusRows,
} from './slash-actions.js'
import { commandItems, fuzzyScore, menuAt, takeCommands, ownCommands } from './composer-menus.js'

// What GET /commands answers for a chat, trimmed to what these tests read.
const commands = [
  { name: 'compact', source: 'builtin', route: 'native', kind: 'command', description: 'Summarize the conversation' },
  { name: 'theme', source: 'builtin', route: 'refuse', kind: 'command', available: false, why: 'only works in the App\'s own terminal window' },
  { name: 'clear', aliases: ['reset'], source: 'builtin', route: 'console', action: 'new', kind: 'command', description: 'Start a new conversation' },
  { name: 'models', source: 'builtin', route: 'console', action: 'model', choices: 'model', kind: 'command' },
  { name: 'review', source: 'builtin', route: 'driver', action: 'review', kind: 'command', argument_hint: '[instructions]' },
  { name: 'security-review', source: 'builtin', route: 'native', kind: 'command' },
  { name: 'ship', source: 'custom', route: 'native', kind: 'command', description: 'Ship it' },
  { name: 'notes:jot', source: 'plugin', route: 'native', kind: 'skill', plugin: 'notes@local' },
  { name: 'mcp__docs__search', source: 'mcp', route: 'native', kind: 'command' },
]

test('the console runs the same actions the daemon tables name', () => {
  const tables = readFileSync(new URL('../daemon/crates/openremote-daemon/src/slash_tables.rs', import.meta.url), 'utf8')
  const block = tables.slice(tables.indexOf('CONSOLE_ACTIONS'), tables.indexOf('];', tables.indexOf('CONSOLE_ACTIONS')))
  const daemon = [...block.matchAll(/"([a-z-]+)"/g)].map((m) => m[1])
  assert.deepEqual(daemon, CONSOLE_ACTIONS)
})

test('a leading command is a word after a slash, never a path', () => {
  assert.deepEqual(leadingCommand('/review the error paths'), { name: 'review', args: 'the error paths' })
  assert.deepEqual(leadingCommand('  /compact'), { name: 'compact', args: '' })
  assert.equal(leadingCommand('/usr/bin is a folder'), null)
  assert.equal(leadingCommand('no command here'), null)
})

test('a terminal-only command is refused with its reason, and the rest pass', () => {
  assert.equal(refusalFor('/theme', commands), "/theme only works in the App's own terminal window")
  assert.equal(refusalFor('/compact', commands), null)
  assert.equal(refusalFor('/unknown-to-us', commands), null)
  assert.equal(refusalFor('just words', commands), null)
})

test("an App's console rows run as OpenRemote's own, under every name they answer to", () => {
  const rows = consoleCommands(commands)
  assert.deepEqual(
    rows.map((r) => [r.name, r.action]),
    [
      ['clear', 'new'],
      ['reset', 'new'],
      ['models', 'model'],
    ],
  )
  // Taken out of the message at send, carrying the action they name.
  const own = [...ownCommands({ inChat: true }), ...rows]
  assert.deepEqual(takeCommands('/reset', own), { run: [{ name: 'reset', action: 'new', arg: null }], text: '' })
  assert.deepEqual(takeCommands('/models opus then go', own), {
    run: [{ name: 'models', action: 'model', arg: 'opus' }],
    text: 'then go',
  })
  // /models opens the model list for its argument, like /model.
  assert.equal(menuAt('/models ', 8, rows)?.arg, 'model')
})

test('fuzzy search finds a command by the starts of its words', () => {
  assert.ok(fuzzyScore('sr', 'security-review'))
  assert.ok(fuzzyScore('secrev', 'security-review'))
  assert.equal(fuzzyScore('deep', 'auto-mode-setup'), 0)
  const labels = commandItems('sr', { harnessCommands: commands }).map((i) => i.label)
  assert.ok(labels.includes('/security-review'), labels.join(' '))
})

test('rows carry a badge for where they came from', () => {
  const badge = (q) => commandItems(q, { harnessCommands: commands }).find((i) => i.label === `/${q}`)?.badge ?? null
  assert.equal(badge('ship'), 'custom')
  assert.equal(badge('mcp__docs__search'), 'MCP')
  assert.equal(badge('compact'), null)
  const jot = commandItems('jot', { harnessCommands: commands }).find((i) => i.label === '/notes:jot')
  assert.equal(jot.badge, 'plugin')
  // A refused row is shown, dimmed, with its reason.
  const theme = commandItems('theme', { harnessCommands: commands }).find((i) => i.label === '/theme')
  assert.equal(theme.disabled, true)
  assert.match(theme.why, /terminal/)
})

test('recent commands are kept per App', () => {
  assert.notEqual(recentKey('codex'), recentKey('claude'))
  assert.equal(recentKey(null), 'openremote-recent-commands')
})

test('a picked command shows what it takes until something is typed', () => {
  assert.deepEqual(pendingHint('/review ', 8, commands), { name: 'review', hint: '[instructions]' })
  assert.equal(pendingHint('/review the', 11, commands), null)
  assert.equal(pendingHint('/compact ', 9, commands), null)
})

test('/help groups every command by where it came from and says how it runs', () => {
  const groups = helpGroups({ own: [{ name: 'model', description: 'Switch the model' }], commands })
  assert.deepEqual(
    groups.map((g) => g.title),
    ['OpenRemote', 'Built in', 'Your commands', 'Skills', 'MCP prompts'],
  )
  const builtIn = groups.find((g) => g.title === 'Built in').rows
  assert.equal(builtIn.find((r) => r.name === '/clear').how, 'Runs in OpenRemote')
  assert.equal(builtIn.find((r) => r.name === '/compact').how, 'Sent to the App')
  assert.equal(builtIn.find((r) => r.name === '/theme').disabled, true)
  assert.deepEqual(
    filterHelp(groups, 'ship').flatMap((g) => g.rows.map((r) => r.name)),
    ['/ship'],
  )
})

test('/status reads what OpenRemote knows about the chat', () => {
  const rows = statusRows(
    { harness: 'codex', model: 'gpt-5', effort: 'high', fast: false, permissionMode: 'on-request', workspace: 'C:/w', status: 'idle', context: { used: 1200, window: 200000 }, costUsd: 0.5 },
    { app: 'Codex', permissionLabel: 'Ask' },
  )
  assert.deepEqual(rows, [
    ['App', 'Codex'],
    ['Model', 'gpt-5'],
    ['Effort', 'high'],
    ['Fast mode', 'Off'],
    ['Permissions', 'Ask'],
    ['Folder', 'C:/w'],
    ['Status', 'idle'],
    ['Context', '1,200 of 200,000 tokens'],
    ['Cost so far', '$0.5000'],
  ])
})

test('/copy takes the newest answer, not a note or the user', () => {
  const chat = {
    timeline: [
      { kind: 'message', role: 'user', text: 'hi' },
      { kind: 'message', role: 'assistant', text: 'first' },
      { kind: 'message', role: 'assistant', text: 'second' },
      { kind: 'message', role: 'note', text: 'compacted' },
      { kind: 'message', role: 'user', text: 'again' },
    ],
  }
  assert.equal(lastAnswer(chat), 'second')
  assert.equal(lastAnswer({ timeline: [] }), null)
})

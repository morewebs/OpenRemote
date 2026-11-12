// The JavaScript plugin runtime's rules: what a record contributes, which
// calls a plugin's powers answer, which messages count as a frame's own, the
// page and policy a frame boots from, and where words land in a composer.

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ALL_CALLS,
  CHAT_CALLS,
  FRAME_SANDBOX,
  contributionsOf,
  frameAllow,
  frameCsp,
  frameDocument,
  frameOf,
  iconName,
  insertAt,
  insidePath,
  isPluginView,
  parseInput,
  parseViewKey,
  refusal,
  ridesProject,
  settingsKey,
  viewKey,
  withDefaults,
} from './plugin-runtime.js'

const record = (over = {}) => ({
  id: 'board@local',
  name: 'board',
  runtime: 'js',
  scope: 'machine',
  enabled: true,
  powers: [],
  entry: 'main.js',
  contributes: {
    composer_inputs: [{ id: 'say', label: 'Say', icon: 'Microphone' }],
    views: [
      { id: 'board', title: 'Board', place: 'sidebar', entry: 'ui/board.html', icon: 'chart-bar' },
      { id: 'hosts', title: 'Hosts', place: 'machines', entry: 'ui/hosts.html' },
    ],
    settings: [
      { key: 'base_url', type: 'string', label: 'Address', default: 'https://api.example.com/v1' },
      { key: 'api_key', type: 'secret', label: 'Key' },
    ],
  },
  ...over,
})

test('a plugin frame is sandboxed to scripts and nothing else', () => {
  assert.equal(FRAME_SANDBOX, 'allow-scripts')
  assert.doesNotMatch(FRAME_SANDBOX, /same-origin|forms|popups|top-navigation|modals|downloads/)
})

test('what a plugin adds is read from its install record', () => {
  const ui = contributionsOf(record())
  assert.equal(ui.entry, 'main.js')
  assert.deepEqual(ui.inputs, [{ id: 'say', label: 'Say', icon: 'microphone', entry: 'main.js' }])
  // Only the sidebar's views are panes here; a view of another place is not
  // drawn as one.
  assert.deepEqual(
    ui.views.map((v) => [v.id, v.entry, v.icon]),
    [['board', 'ui/board.html', 'chartbar']],
  )
  assert.deepEqual(
    ui.settings.map((s) => [s.key, s.type]),
    [
      ['base_url', 'string'],
      ['api_key', 'secret'],
    ],
  )
  // A button with no page to run is never drawn.
  assert.deepEqual(contributionsOf(record({ entry: undefined })).inputs, [])
  // A page that climbs out of the folder is no page.
  assert.equal(contributionsOf(record({ entry: '../outside.js' })).entry, null)
  assert.deepEqual(contributionsOf({}), { entry: null, inputs: [], views: [], settings: [] })
})

test('an icon is one of the console\'s own, else the plugin piece', () => {
  assert.equal(iconName('Microphone'), 'microphone')
  assert.equal(iconName('magic-wand'), 'magicwand')
  assert.equal(iconName('MagicWand'), 'magicwand')
  assert.equal(iconName('ShieldCheck'), 'puzzlepiece')
  assert.equal(iconName(null), 'puzzlepiece')
})

test('a path is inside the plugin only when it never climbs out', () => {
  for (const good of ['main.js', 'ui/view.html', 'a/b/c.png']) assert.ok(insidePath(good), good)
  for (const bad of ['', '/etc/passwd', '../x', 'a/../../x', 'C:/x', 'a\\b', 'a//b', './a', null, 42]) {
    assert.equal(insidePath(bad), false, String(bad))
  }
})

test('the calls are a closed list, each gated by the power the record holds', () => {
  const none = { name: 'Board', powers: [], inputs: [] }
  // Not on the list: refused whatever the plugin was given.
  const every = { name: 'Board', powers: ['read_chats', 'agent_tools', 'network', 'microphone', 'run_programs'], inputs: [{ id: 'x' }] }
  for (const call of ['sessions.create', 'plugins.remove', 'settings.set', 'eval', '__proto__', 'constructor', 'toString', '', undefined]) {
    assert.match(refusal(call, every) ?? '', /no openremote call/, String(call))
  }
  // On the list: each power from the record.
  for (const [call, power] of [
    ['chats.list', 'read_chats'],
    ['chats.read', 'read_chats'],
    ['chats.start', 'agent_tools'],
    ['chats.steer', 'agent_tools'],
    ['chats.stop', 'agent_tools'],
    ['fetch', 'network'],
  ]) {
    assert.match(refusal(call, none), /not approved/, call)
    assert.equal(refusal(call, { ...none, powers: [power] }), null, call)
  }
  // The composer only for a plugin that adds a composer button.
  assert.match(refusal('composer.insert', none), /adds nothing to the composer/)
  assert.equal(refusal('composer.insert', { ...none, inputs: [{ id: 'say' }] }), null)
  // A plugin's own things need no power.
  for (const call of ['settings.get', 'files.read', 'ui.toast', 'ui.openView', 'ui.openSettings']) {
    assert.equal(refusal(call, none), null, call)
  }
  // The chat calls reach exactly the daemon's five methods.
  assert.deepEqual(Object.values(CHAT_CALLS).sort(), ['chats/list', 'chats/prompt', 'chats/read', 'chats/start', 'chats/stop'])
  assert.equal(ALL_CALLS.size, 14)
})

test('a message counts only from a frame the host made, with the opaque origin', () => {
  const win = { name: 'frame window' }
  const frame = { win, pluginId: 'board@local' }
  const frames = new Map([[win, frame]])
  const ok = { source: win, origin: 'null', data: { openremote: 1, kind: 'call', id: 1, method: 'ui.toast' } }
  assert.equal(frameOf(ok, frames), frame)
  // Another window, even with the right shape and origin.
  assert.equal(frameOf({ ...ok, source: { name: 'another' } }, frames), null)
  assert.equal(frameOf({ ...ok, source: null }, frames), null)
  // The right window with an origin of its own: it is no longer sandboxed
  // as it was made, so it is not answered.
  for (const origin of ['http://tauri.localhost', 'https://evil.example', '', undefined, 'null ']) {
    assert.equal(frameOf({ ...ok, origin }, frames), null, String(origin))
  }
  // The bridge's own shape only.
  for (const data of [
    null,
    'call',
    { kind: 'call', id: 1, method: 'x' },
    { openremote: 2, kind: 'call', id: 1, method: 'x' },
    { openremote: 1, kind: 'reply', id: 1 },
    { openremote: 1, kind: 'call', id: '1', method: 'x' },
    { openremote: 1, kind: 'call', id: 1 },
  ]) {
    assert.equal(frameOf({ ...ok, data }, frames), null, JSON.stringify(data))
  }
  assert.equal(frameOf(ok, null), null)
})

test("a frame's page names the console's origin and its bridge, never the token or the daemon", () => {
  const page = frameDocument('http://tauri.localhost')
  assert.match(page, /<script src="http:\/\/tauri\.localhost\/plugin-bridge\.js"><\/script>/)
  assert.doesNotMatch(page, /token|Bearer|Authorization|127\.0\.0\.1|localStorage/i)
  // The frame's own policy: nothing of its own to reach, scripts only from
  // the console's bridge and what it makes inside itself.
  const policy = Object.fromEntries(
    frameCsp('http://tauri.localhost')
      .split(';')
      .map((d) => d.trim().split(/\s+/))
      .map(([name, ...sources]) => [name, sources]),
  )
  assert.deepEqual(policy['default-src'], ["'none'"])
  assert.deepEqual(policy['connect-src'], ["'none'"])
  assert.deepEqual(policy['frame-src'], ["'none'"])
  assert.deepEqual(policy['form-action'], ["'none'"])
  assert.deepEqual(policy['script-src'], ['http://tauri.localhost', 'blob:'])
  assert.ok(!Object.values(policy).flat().some((s) => s === '*' || /^https?:\/\/(?!tauri\.localhost$)/.test(s)))
  // The origin is written as text, never as markup.
  assert.doesNotMatch(frameDocument('x"><script>alert(1)</script>'), /"><script>alert/)
})

test('the microphone is delegated only to a plugin given it', () => {
  assert.equal(frameAllow([]), undefined)
  assert.equal(frameAllow(['network']), undefined)
  assert.equal(frameAllow(['microphone']), 'microphone *')
  assert.equal(frameAllow(undefined), undefined)
})

test("a project's plugin is for its project's chats only", () => {
  assert.equal(ridesProject({ scope: 'machine' }, null), true)
  assert.equal(ridesProject({ scope: 'global' }, 'p1'), true)
  for (const scope of ['project', 'project_local']) {
    assert.equal(ridesProject({ scope, project: 'p1' }, 'p1'), true)
    assert.equal(ridesProject({ scope, project: 'p1' }, 'p2'), false)
    assert.equal(ridesProject({ scope, project: 'p1' }, null), false)
    assert.equal(ridesProject({ scope, project: null }, null), false, 'a malformed record rides nothing')
  }
})

test('a view key round-trips, ids with a project included', () => {
  const id = 'board@local~6c1f'
  assert.equal(viewKey(id, 'main'), 'plugin:board@local~6c1f/main')
  assert.deepEqual(parseViewKey(viewKey(id, 'main')), { pluginId: id, viewId: 'main' })
  assert.equal(isPluginView('plugin:x/y'), true)
  assert.equal(isPluginView('new'), false)
  assert.equal(parseViewKey('plugin:/y'), null)
  assert.equal(parseViewKey('plugin:x/'), null)
  assert.equal(settingsKey('board@local~6c1f'), 'plugin-board-local-6c1f')
})

test('a plugin reads every setting it declares and never a secret', () => {
  const { settings } = contributionsOf(record())
  const read = withDefaults({ values: {}, secrets: { api_key: 'set' }, api_key: 'sk-leak' }, settings)
  assert.deepEqual(read, { values: { base_url: 'https://api.example.com/v1' }, secrets: { api_key: 'set' } })
  // A secret value the answer somehow carried stays out.
  const odd = withDefaults({ values: { api_key: 'sk-leak' }, secrets: { api_key: 'sk-leak' } }, settings)
  assert.equal(JSON.stringify(odd).includes('sk-leak'), false)
  assert.deepEqual(withDefaults(null, settings).secrets, { api_key: 'unset' })
})

test('a field means its setting', () => {
  const number = { key: 'n', type: 'number', label: 'N', min: 1, max: 30 }
  assert.deepEqual(parseInput(number, '5'), { value: 5 })
  assert.deepEqual(parseInput(number, ''), { value: null })
  assert.match(parseInput(number, '0').error, /at least 1/)
  assert.match(parseInput(number, 'x').error, /takes a number/)
  const choice = { key: 'c', type: 'enum', label: 'C', options: [{ value: 'a', label: 'A' }] }
  assert.deepEqual(parseInput(choice, 'a'), { value: 'a' })
  assert.match(parseInput(choice, 'b').error, /not one of/)
})

test('words go in at the cursor with a space where they would touch', () => {
  assert.deepEqual(insertAt('hello', 5, 5, 'world'), { text: 'hello world', caret: 11 })
  assert.deepEqual(insertAt('hello there', 6, 6, 'big'), { text: 'hello big there', caret: 10 })
  assert.deepEqual(insertAt('', 0, 0, 'hi'), { text: 'hi', caret: 2 })
  assert.deepEqual(insertAt('a  b', 2, 2, 'x'), { text: 'a x b', caret: 3 })
  assert.deepEqual(insertAt('replace me', 0, 7, 'keep'), { text: 'keep me', caret: 4 })
})

test("a plugin view passes up only the app's moving-around keys, and only while focused", async () => {
  const { keyPassesUp } = await import('./plugin-runtime.js')
  const { CATALOG } = await import('./shortcuts.js')
  const action = (id) => CATALOG.find((a) => a.id === id)
  const focusedView = { view: true, focused: true }
  for (const id of ['new-chat', 'palette', 'toggle-sidebar', 'split-right', 'focus-pane-2']) {
    assert.equal(keyPassesUp(action(id), focusedView), true, id)
  }
  // A key that acts on a chat is never pressed for the user.
  for (const id of ['steer', 'stop', 'archive', 'interrupt', 'pick-permission', 'toggle-fast', 'browser-close-tab']) {
    assert.equal(keyPassesUp(action(id), focusedView), false, id)
  }
  // Nor from a hidden page, or a view without the focus, or for no action.
  assert.equal(keyPassesUp(action('new-chat'), { view: false, focused: true }), false)
  assert.equal(keyPassesUp(action('new-chat'), { view: true, focused: false }), false)
  assert.equal(keyPassesUp(undefined, focusedView), false)
})

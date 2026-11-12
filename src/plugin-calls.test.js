// The console's answers to a plugin frame's calls: refused unless the
// install record allows them, carried to the daemon for the chats, never
// carrying the token back, and never writing in a composer the user did not
// click the plugin's button in.

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  GRANT_MS,
  answerCall,
  composerFor,
  composerGrants,
  dropGrants,
  forgetComposer,
  grantClick,
  makeCalls,
} from './plugin-calls.js'
import { contributionsOf } from './plugin-runtime.js'

const TOKEN = 'daemon-token-0f7c'

function record(powers = [], over = {}) {
  const plugin = {
    id: 'board@local',
    name: 'board',
    runtime: 'js',
    scope: 'machine',
    enabled: true,
    powers,
    entry: 'main.js',
    contributes: {
      composer_inputs: [{ id: 'say', label: 'Say' }],
      views: [{ id: 'board', title: 'Board', place: 'sidebar', entry: 'ui/board.html' }],
      settings: [{ key: 'api_key', type: 'secret', label: 'Key' }],
    },
    ...over,
  }
  return { plugin, ui: contributionsOf(plugin), name: 'Board' }
}

// A stand-in for the console's DaemonApi: it holds the token, as the real
// one does, and notes every request the console made for the frame.
function fakeApi() {
  const asked = []
  const api = {
    token: TOKEN,
    url: 'http://127.0.0.1:7777',
    async pluginCall(id, method, params) {
      asked.push(['call', id, method, params])
      return { result: { chats: [{ id: 'c1', title: 'One' }] } }
    },
    async pluginSettings(id) {
      asked.push(['settings', id])
      return { id, values: {}, secrets: { api_key: 'set' } }
    },
    async pluginFile(id, path) {
      asked.push(['file', id, path])
      return { type: 'text/plain', bytes: new TextEncoder().encode('hello').buffer }
    },
    async pluginFetch(id, request) {
      asked.push(['fetch', id, request])
      return { status: 200, headers: [['content-type', 'text/plain']], body: btoa('ok') }
    },
  }
  return { api, asked }
}

function setup() {
  const { api, asked } = fakeApi()
  const clock = { t: 1_000_000 }
  const grants = composerGrants(() => clock.t)
  const toasts = []
  const states = {}
  const calls = makeCalls({
    api: () => api,
    toast: (name, text) => toasts.push([name, text]),
    openView: () => {},
    openSettings: () => {},
    setInputState: (key, state) => {
      states[key] = state
    },
    attach: () => {},
    grants,
  })
  return { api, asked, grants, toasts, states, calls, clock }
}

const call = (method, params = {}, id = 1) => ({ openremote: 1, kind: 'call', id, method, params })

test('a call off the list, or past the record, asks the daemon nothing', async () => {
  const { asked, calls } = setup()
  const every = record(['read_chats', 'agent_tools', 'network', 'microphone', 'run_programs'])
  for (const method of ['sessions.create', 'plugins.remove', '__proto__', 'constructor', 'hasOwnProperty', 'eval']) {
    const reply = await answerCall(every, call(method), calls)
    assert.match(reply.error, /no openremote call/, method)
  }
  // The record holds no power: every gated call is refused before a request.
  const bare = record([])
  for (const method of ['chats.list', 'chats.read', 'chats.start', 'chats.steer', 'chats.stop', 'fetch']) {
    const reply = await answerCall(bare, call(method, { id: 'c1', url: 'https://x.example' }), calls)
    assert.match(reply.error, /not approved/, method)
  }
  // Off answers nothing at all.
  const off = record(['read_chats'], { enabled: false })
  assert.match((await answerCall(off, call('chats.list'), calls)).error, /turned off/)
  assert.deepEqual(asked, [])
})

test('the chat calls go to the daemon, which keeps the record and the project', async () => {
  const { asked, calls } = setup()
  const reader = record(['read_chats', 'agent_tools'])
  const listed = await answerCall(reader, call('chats.list'), calls)
  assert.deepEqual(listed.result, { chats: [{ id: 'c1', title: 'One' }] })
  await answerCall(reader, call('chats.read', { id: 'c1' }), calls)
  await answerCall(reader, call('chats.steer', { id: 'c1', text: 'go on' }), calls)
  await answerCall(reader, call('chats.stop', { id: 'c1' }), calls)
  await answerCall(reader, call('chats.start', { text: 'hi', harness: 'claude' }), calls)
  assert.deepEqual(
    asked.map(([kind, id, method, params]) => [kind, id, method, params]),
    [
      ['call', 'board@local', 'chats/list', {}],
      ['call', 'board@local', 'chats/read', { chat: 'c1' }],
      ['call', 'board@local', 'chats/prompt', { chat: 'c1', text: 'go on' }],
      ['call', 'board@local', 'chats/stop', { chat: 'c1' }],
      ['call', 'board@local', 'chats/start', { harness: 'claude', workspace: undefined, model: undefined, prompt: 'hi' }],
    ],
  )
})

test('no answer a frame gets carries the token', async () => {
  const { calls, grants } = setup()
  const all = record(['read_chats', 'agent_tools', 'network'])
  grants.composers.set(':h1:', { insert: () => {}, attachKey: 'c1' })
  grantClick(grants, 'board@local', ':h1:')
  const replies = []
  for (const message of [
    call('chats.list'),
    call('chats.read', { id: 'c1' }),
    call('settings.get'),
    call('files.read', { path: 'main.js' }),
    call('fetch', { url: 'https://api.example.com', headers: [['authorization', 'Bearer {{secret:api_key}}']] }),
    call('composer.insert', { text: 'words' }),
    call('ui.toast', { text: 'hi' }),
    call('nope'),
  ]) {
    const reply = await answerCall(all, message, calls)
    const { transfer, ...payload } = reply
    replies.push(JSON.stringify(payload, (_, v) => (v instanceof ArrayBuffer ? new TextDecoder().decode(v) : v)))
  }
  for (const text of replies) {
    assert.ok(!text.includes(TOKEN), text)
    assert.ok(!text.includes('127.0.0.1'), text)
  }
  // settings.get hands over set or unset, never a value.
  assert.match(replies[2], /"api_key":"set"/)
})

test('a plugin writes in a composer only after the user clicks its button there', async () => {
  const { calls, grants } = setup()
  const writer = record([])
  const typed = { a: [], b: [] }
  grants.composers.set(':a:', { insert: (t) => typed.a.push(t), attachKey: 'chat-a' })
  grants.composers.set(':b:', { insert: (t) => typed.b.push(t), attachKey: 'chat-b' })

  // No click yet: nothing goes in anywhere, named or not.
  for (const composer of [null, ':a:', ':b:']) {
    const reply = await answerCall(writer, call('composer.insert', { text: 'x', composer }), calls)
    assert.match(reply.error, /only after the user clicks/)
  }
  assert.throws(() => composerFor(grants, 'board@local', null), /only after the user clicks/)

  // A click in composer a: a, and only a.
  grantClick(grants, 'board@local', ':a:')
  assert.equal((await answerCall(writer, call('composer.insert', { text: 'hello' }), calls)).error, undefined)
  assert.equal((await answerCall(writer, call('composer.insert', { text: 'more', composer: ':a:' }), calls)).error, undefined)
  const toB = await answerCall(writer, call('composer.insert', { text: 'sneak', composer: ':b:' }), calls)
  assert.match(toB.error, /only after the user clicks/)
  assert.deepEqual(typed, { a: ['hello', 'more'], b: [] })

  // Another plugin's click grants this one nothing.
  const other = { ...record([]), plugin: { ...record([]).plugin, id: 'other@local' } }
  grantClick(grants, 'other@local', ':b:')
  assert.match((await answerCall(writer, call('composer.insert', { text: 'x', composer: ':b:' }), calls)).error, /only after/)
  assert.equal((await answerCall(other, call('composer.insert', { text: 'theirs' }), calls)).error, undefined)
  assert.deepEqual(typed.b, ['theirs'])

  // The composer went away (or now shows another chat): the grant goes
  // with it, and a new composer for the same chat holds none.
  grants.composers.delete(':a:')
  forgetComposer(grants, ':a:')
  grants.composers.set(':a2:', { insert: (t) => typed.a.push(t), attachKey: 'chat-a' })
  assert.match((await answerCall(writer, call('composer.insert', { text: 'back' }), calls)).error, /only after the user clicks/)
  assert.deepEqual(typed.a, ['hello', 'more'])

  // Turned off or removed: its grants go.
  dropGrants(grants, new Set(['other@local']))
  assert.throws(() => composerFor(grants, 'board@local', null), /only after the user clicks/)

  // A plugin with no composer button may not write at all.
  const noButton = record([], { contributes: { composer_inputs: [] } })
  assert.match((await answerCall(noButton, call('composer.insert', { text: 'x' }), calls)).error, /adds nothing to the composer/)
})

test('a button state names a composer only where the user clicked', async () => {
  const { calls, grants, states } = setup()
  const writer = record([])
  grants.composers.set(':a:', { insert: () => {}, attachKey: 'chat-a' })
  await answerCall(writer, call('composer.setState', { input: 'say', state: 'active', composer: ':a:' }), calls)
  assert.equal(states['board@local/say'].composer, null, 'not clicked there: every composer, no grant implied')
  grantClick(grants, 'board@local', ':a:')
  await answerCall(writer, call('composer.setState', { input: 'say', state: 'busy', label: 'x'.repeat(90), composer: ':a:' }), calls)
  assert.equal(states['board@local/say'].composer, ':a:')
  assert.equal(states['board@local/say'].label.length, 40)
  const unknown = await answerCall(writer, call('composer.setState', { input: 'nope' }), calls)
  assert.match(unknown.error, /no composer button named nope/)
})

test("a file read stays inside the plugin's folder", async () => {
  const { asked, calls } = setup()
  for (const path of ['../secrets', '/etc/passwd', 'C:/x', 'a\\b']) {
    const reply = await answerCall(record([]), call('files.read', { path }), calls)
    assert.match(reply.error, /not a path inside/, path)
  }
  assert.deepEqual(asked, [])
})

test('a grant ends when the button goes back to idle, or five minutes after the click', async () => {
  const { calls, grants, clock } = setup()
  const writer = record([])
  const typed = []
  grants.composers.set(':a:', { insert: (t) => typed.push(t), attachKey: 'chat-a' })
  const insert = (text) => answerCall(writer, call('composer.insert', { text, composer: ':a:' }), calls)

  // Idle: the work the click asked for is done, and so is the grant.
  grantClick(grants, 'board@local', ':a:', 'say')
  await answerCall(writer, call('composer.setState', { input: 'say', state: 'busy', composer: ':a:' }), calls)
  assert.equal((await insert('one')).error, undefined)
  await answerCall(writer, call('composer.setState', { input: 'say', state: 'idle', composer: ':a:' }), calls)
  assert.match((await insert('two')).error, /only after the user clicks/)
  // An idle naming no composer ends every grant of that button.
  grantClick(grants, 'board@local', ':a:', 'say')
  await answerCall(writer, call('composer.setState', { input: 'say', state: 'idle' }), calls)
  assert.match((await insert('three')).error, /only after the user clicks/)
  // A state that isn't one reads as idle, and ends it too.
  grantClick(grants, 'board@local', ':a:', 'say')
  await answerCall(writer, call('composer.setState', { input: 'say', state: 'whatever' }), calls)
  assert.match((await insert('four')).error, /only after the user clicks/)

  // Time: a click counts for five minutes and not a moment more.
  grantClick(grants, 'board@local', ':a:', 'say')
  clock.t += GRANT_MS - 1
  assert.equal((await insert('five')).error, undefined)
  clock.t += 1
  assert.match((await insert('six')).error, /only after the user clicks/)
  assert.equal(GRANT_MS, 5 * 60 * 1000)
  assert.deepEqual(typed, ['one', 'five'])
})

test("a plugin's picture is held to the composer's own 5 MB, as the daemon holds it", async () => {
  const { MAX_ATTACH_BYTES } = await import('./plugin-calls.js')
  const { MAX_IMAGE_BYTES } = await import('./composer-attachments.js')
  assert.equal(MAX_ATTACH_BYTES, MAX_IMAGE_BYTES)
  const { calls, grants } = setup()
  const writer = record([])
  grants.composers.set(':a:', { insert: () => {}, attachKey: 'chat-a' })
  grantClick(grants, 'board@local', ':a:')
  const attach = (size) =>
    answerCall(writer, call('composer.attach', { name: 'p.png', mime: 'image/png', bytes: new ArrayBuffer(size), composer: ':a:' }), calls)
  assert.equal((await attach(MAX_IMAGE_BYTES)).error, undefined)
  assert.match((await attach(MAX_IMAGE_BYTES + 1)).error, /up to 5 MB/)
  const { readFileSync } = await import('node:fs')
  const doc = readFileSync(new URL('../docs/plugins/js-api.md', import.meta.url), 'utf8')
  assert.match(doc, /`composer\.attach\(file, \{ composer, name \}\)` \| Adds a picture \(PNG, JPEG, GIF or WebP, up to 5 MB;/)
})

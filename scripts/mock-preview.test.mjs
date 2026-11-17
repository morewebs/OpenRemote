import test from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { createMockBackend } from './mock-preview/backend.mjs'
import { DaemonApi } from '../src/api/daemon.js'

async function fixture(t) {
  const backend = createMockBackend()
  const server = createServer(backend.handle)
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  t.after(() => { backend.close(); server.closeAllConnections(); server.close() })
  const origin = `http://127.0.0.1:${server.address().port}`
  return { backend, origin, api: new DaemonApi(origin, 'mock-preview') }
}

test('mock backend satisfies the real console boot and model clients', async t => {
  const { api } = await fixture(t)
  assert.equal((await api.health()).ok, true)
  assert.ok((await api.capabilities()).harnesses.some(h => h.available))
  assert.ok((await api.sessions()).length > 0)
  assert.ok((await api.projects()).length > 0)
  assert.ok(Array.isArray(await api.machines()))
  assert.ok(Array.isArray(await api.mcpServers()))
  assert.ok(Array.isArray(await api.automations()))
  assert.ok((await api.models('claude')).some(m => m.is_default))
  assert.ok(Array.isArray((await api.plugins()).installed))
  assert.ok((await api.plugins()).builtin.length > 0)
  assert.ok(Array.isArray((await api.agentSettings()).harnesses))
  assert.ok(Array.isArray((await api.fsDirs()).dirs))
  await api.setAgentSettings({ max_depth: 3 })
  assert.equal((await api.agentSettings()).max_depth, 3)
})

test('chat mutations and SSE work without an agent, with state reset between scenarios', async t => {
  const { api, origin } = await fixture(t)
  const chat = await api.createSession({ harness: 'claude', workspace: '/preview/openremote' })
  const events = []
  const stop = api.events(chat.id, { onEvent: event => events.push(event) })
  t.after(stop)
  await api.prompt(chat.id, 'Test the composer')
  const until = Date.now() + 3000
  while (!events.some(e => e.type === 'message.added' && e.message.role === 'assistant') && Date.now() < until) {
    await new Promise(resolve => setTimeout(resolve, 20))
  }
  assert.ok(events.some(e => e.type === 'message.added' && e.message.text === 'Test the composer'))
  assert.ok(events.some(e => e.type === 'message.added' && e.message.role === 'assistant'))
  await api.rename(chat.id, 'Renamed preview')
  assert.equal((await api.session(chat.id)).title, 'Renamed preview')
  await api.deleteSession(chat.id)
  assert.ok(!(await api.sessions()).some(s => s.id === chat.id))
  const reset = await fetch(`${origin}/__preview/reset`, { method: 'POST', body: JSON.stringify({ scenario: 'empty' }) })
  assert.equal(reset.status, 200)
  assert.deepEqual(await api.sessions(), [])
  assert.deepEqual(await api.projects(), [])
})

test('unsupported routes fail explicitly and scenarios recover cleanly', async t => {
  const { api, origin } = await fixture(t)
  assert.equal((await fetch(`${origin}/not-implemented`)).status, 501)
  await fetch(`${origin}/__preview/reset`, { method: 'POST', body: JSON.stringify({ scenario: 'error' }) })
  await assert.rejects(api.health(), /Simulated/)
  await fetch(`${origin}/__preview/reset`, { method: 'POST', body: JSON.stringify({ scenario: 'default' }) })
  assert.equal((await api.health()).ok, true)
})

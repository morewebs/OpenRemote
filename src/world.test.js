import assert from 'node:assert/strict'
import test from 'node:test'
import {
  acknowledgePluginKey,
  addAutomation,
  addCustomPlugin,
  addDevice,
  addStarter,
  contextWindow,
  installFromCatalog,
  installOnDevice,
  openChat,
  removeAutomation,
  removePlugin,
  resetWorld,
  resolveTool,
  runAutomation,
  seedWorld,
  sendMessage,
  setDeviceOnline,
  setPluginEnabled,
  signInHarness,
  updateAutomation,
  usageFor,
} from './world.js'
import { whenSentence } from './automation.js'

test('seed sessions are specific and the windows build is waiting on a real command', () => {
  const world = seedWorld()
  const users = world.chats.map((c) => c.messages.find((m) => m.role === 'user').text)
  assert.equal(new Set(users).size, users.length)
  const hang = world.chats.find((c) => c.id === 'build-hang')
  const tool = hang.messages.find((m) => m.role === 'tool')
  assert.equal(hang.status, 'waiting')
  assert.equal(tool.state, 'pending')
  assert.equal(tool.arg, 'cargo build -vv --release')
  assert.match(world.chats.find((c) => c.id === 'dark-mode').messages.at(-1).text, /tokens/)
  assert.equal(world.chats.some((c) => c.messages.some((m) => /Looking at/.test(m.text))), false)
})

test('allowing the link build records its result and deny leaves the hang', () => {
  const hang = seedWorld().chats.find((c) => c.id === 'build-hang')
  const toolId = hang.messages.find((m) => m.role === 'tool').id
  const allowed = resolveTool(seedWorld(), 'build-hang', toolId, 'once')
  const done = allowed.chats.find((c) => c.id === 'build-hang')
  assert.equal(done.status, 'idle')
  assert.match(done.messages.find((m) => m.id === toolId).result, /mspdbsrv/)
  const denied = resolveTool(seedWorld(), 'build-hang', toolId, 'deny')
  const refused = denied.chats.find((c) => c.id === 'build-hang')
  assert.match(refused.messages.find((m) => m.id === toolId).result, /link step/)
  assert.equal(refused.messages.some((m) => m.role === 'agent' && /link step/.test(m.text)), false)
  const session = resolveTool(seedWorld(), 'build-hang', toolId, 'session')
  assert.deepEqual(session.chats.find((c) => c.id === 'build-hang').approvals, ['shell'])
})

test('a reply is kept and the harness is what the session waits on', () => {
  const world = sendMessage(seedWorld(), 'dark-mode', 'the account page is still light')
  const chat = world.chats.find((c) => c.id === 'dark-mode')
  assert.equal(chat.status, 'running')
  assert.equal(chat.messages.at(-1).text, 'the account page is still light')
  assert.equal(chat.messages.at(-1).role, 'user')
  assert.equal(chat.messages.some((m) => m.role === 'agent' && m.text.includes('account page')), false)
})

test('a full session compacts before the next message', () => {
  const reply = 'the payments origin is the slow one'
  const world = sendMessage(seedWorld(), 'edge-502', reply)
  const chat = world.chats.find((c) => c.id === 'edge-502')
  assert.ok(chat.messages.some((m) => m.role === 'note' && m.text.includes('92%')))
  assert.equal(chat.contextUsed, Math.round(0.4 * contextWindow('auto')) + usageFor(reply))
})

test('cloud work lands on a machine only when that harness is installed there', () => {
  const base = {
    text: 'ship the notes',
    harness: 'cx',
    model: 'gpt6',
    modelLabel: 'GPT-6',
    project: 'site',
    deviceId: 'server',
  }
  const missing = openChat(seedWorld(), { ...base, mode: 'cloud' })
  assert.equal(missing.chats[0].status, 'idle')
  assert.match(missing.chats[0].messages.at(-1).text, /not installed on server/)
  assert.equal(missing.devices.find((d) => d.id === 'server').agents.some((a) => a.live), false)
  let world = installOnDevice(seedWorld(), 'server', 'cx')
  world = openChat(world, { ...base, mode: 'cloud' })
  assert.equal(world.chats[0].status, 'running')
  assert.equal(world.chats[0].machine, 'server')
  assert.ok(world.devices.find((d) => d.id === 'server').agents.some((a) => a.chatId === world.chats[0].id))
  const local = openChat(seedWorld(), { ...base, mode: 'local' })
  assert.equal(local.chats[0].machine, null)
  assert.equal(local.chats[0].status, 'running')
})

test('an offline machine keeps the task instead of pretending to start it', () => {
  const world = openChat(setDeviceOnline(seedWorld(), 'mac-mini', false), {
    text: 'rename the signing key',
    harness: 'cc',
    model: 'sonnet',
    modelLabel: 'Sonnet 5.5',
    deviceId: 'mac-mini',
    mode: 'cloud',
    project: 'payments',
  })
  assert.equal(world.chats[0].status, 'idle')
  assert.match(world.chats[0].messages.at(-1).text, /offline/)
})

test('adding a machine waits for it, and a rule opens a real task', () => {
  let world = addDevice(seedWorld(), { os: 'linux', name: 'build-box' })
  const added = world.devices.find((d) => d.id === 'build-box')
  assert.equal(added.status, 'waiting')
  assert.equal(added.detail, 'Waiting for the agent')
  assert.equal(addDevice(world, { os: 'linux', name: 'build-box' }).devices.length, world.devices.length)
  world = signInHarness(world, 'od')
  const ran = runAutomation(world, 'webhook', Date.parse('2026-10-01T08:00:00Z'))
  assert.equal(ran.chatId, null)
  const fired = runAutomation(world, 'ci-main', Date.parse('2026-10-01T08:00:00Z'))
  assert.ok(fired.chatId)
  const chat = fired.world.chats.find((c) => c.id === fired.chatId)
  assert.match(chat.messages[0].text, /link step/)
  assert.equal(chat.project, 'api')
  assert.equal(chat.machine, 'server')
  const keyed = acknowledgePluginKey(setPluginEnabled(acknowledgePluginKey(seedWorld(), 'postgres@server'), 'postgres@server', false), 'postgres@server')
  assert.equal(keyed.plugins.find((p) => p.id === 'postgres@server').hasKey, true)
  assert.equal(setPluginEnabled(keyed, 'postgres@server', false).plugins.find((p) => p.id === 'postgres@server').deviceId, 'server')
  assert.equal(resetWorld().devices.some((d) => d.id === 'build-box'), false)
})

test('the marketplace installs onto a machine and a custom plugin needs a command', () => {
  const seed = seedWorld()
  assert.equal(seed.plugins.some((p) => p.catalogId === 'browser'), false)
  const browser = installFromCatalog(seed, 'browser', 'mac-mini')
  const row = browser.plugins.find((p) => p.id === 'browser@mac-mini')
  assert.equal(row.status, 'running')
  assert.equal(row.deviceId, 'mac-mini')
  assert.equal(installFromCatalog(browser, 'browser', 'mac-mini').plugins.length, browser.plugins.length)
  assert.equal(installFromCatalog(seed, 'browser', 'thinkpad'), seed)
  const again = installFromCatalog(browser, 'browser', 'server')
  assert.equal(again.plugins.filter((p) => p.catalogId === 'browser').length, 2)
  const custom = addCustomPlugin(seed, { name: 'Notes', detail: 'Read the notes repo', command: 'npx -y notes-mcp', deviceId: 'mac-mini' })
  assert.equal(custom.plugins.at(-1).catalogId, null)
  assert.equal(custom.plugins.at(-1).status, 'running')
  assert.equal(addCustomPlugin(seed, { name: 'Notes', detail: 'x', command: '', deviceId: 'mac-mini' }), seed)
  assert.equal(addCustomPlugin(custom, { name: 'Notes', detail: 'x', command: 'npx notes', deviceId: 'mac-mini' }), custom)
  const removed = removePlugin(custom, custom.plugins.at(-1).id)
  assert.equal(removed.plugins.some((p) => p.name === 'Notes'), false)
  assert.equal(installFromCatalog(removed, 'browser', 'mac-mini').plugins.some((p) => p.id === 'browser@mac-mini'), true)
})

test('a starter becomes one rule, and editing keeps the last chat', () => {
  assert.equal(whenSentence({ kind: 'schedule', time: '09:00' }), 'Weekdays at 09:00')
  assert.equal(whenSentence({ kind: 'pipeline', project: 'api', branch: 'main' }), 'The api pipeline on main fails')
  let world = addStarter(seedWorld(), 'deps')
  const rule = world.automations.find((a) => a.starterId === 'deps')
  assert.equal(rule.when, 'Weekdays at 09:00')
  assert.equal(rule.enabled, true)
  assert.equal(rule.lastChatId, null)
  assert.equal(addStarter(world, 'deps').automations.length, world.automations.length)
  world = updateAutomation(world, 'ci-main', {
    name: 'Main went red',
    task: 'The release build on Windows is hanging in the link step. Find the lock and clear it.',
    trigger: { kind: 'pipeline', project: 'api', branch: 'main' },
    harness: 'cc',
    model: 'opus',
    modelLabel: 'Opus 5.5',
    deviceId: 'server',
    project: 'api',
    enabled: true,
  })
  const edited = world.automations.find((a) => a.id === 'ci-main')
  assert.equal(edited.lastChatId, 'build-hang')
  assert.equal(edited.when, 'The api pipeline on main fails')
  const blank = addAutomation(world, { name: 'Nightly', task: 'Look at the lockfile', trigger: { kind: 'schedule', time: '07:30' }, harness: 'cc', model: 'sonnet', modelLabel: 'Sonnet 5.5', deviceId: 'mac-mini', project: 'webapp' })
  const created = blank.automations[0]
  assert.equal(created.when, 'Weekdays at 07:30')
  assert.equal(created.starterId, null)
  world = removeAutomation(blank, rule.id)
  assert.equal(world.automations.some((a) => a.starterId === 'deps'), false)
  assert.ok(addStarter(world, 'deps').automations.some((a) => a.starterId === 'deps'))
})

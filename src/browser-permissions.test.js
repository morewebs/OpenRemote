// The permission bar (ported from the browser branch's 0d3211a and the
// site-permission half of 015ea58): a site's request is asked in an info
// bar in the pane's flow (Allow, Block, remember for the site), a
// remembered answer is given at once, the site menu and Settings > Browser
// list and reset them, and a private tab always asks and remembers nothing.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { hostOf, permissionAsk, permissionName, permissionRows, rememberedAnswer } from './browser-chrome.js'
import { getBrowserPrefs, loadBrowserPrefs, setSitePermission } from './browser-prefs.js'
import { answerPrompt, dropPromptsFor, getState, onPermission, setState } from './browser-store.js'
import { SETTINGS_ENTRIES, searchSettings } from './settings-search.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('the bar says who asks for what, in words', () => {
  assert.equal(hostOf('https://meet.example.com/room'), 'meet.example.com')
  assert.equal(hostOf('not a url'), '')
  assert.equal(permissionAsk('camera'), 'use your camera')
  assert.equal(permissionAsk('geolocation'), 'know your location')
  assert.equal(permissionAsk('fonts', 'fonts'), 'use fonts')
  assert.equal(permissionAsk('other'), 'use a feature of this computer')
  assert.equal(permissionName('microphone'), 'Microphone')
  assert.equal(permissionName('window-management'), 'Window management')
})

test('remembered answers list by site, and only allow or deny count', () => {
  const kept = {
    'https://b.test|microphone': 'deny',
    'https://a.test|camera': 'allow',
    'https://a.test|geolocation': 'allow',
    'https://c.test|camera': 'maybe',
  }
  assert.deepEqual(permissionRows(kept).map((r) => `${r.origin} ${r.kind} ${r.state}`), [
    'https://a.test camera allow',
    'https://a.test geolocation allow',
    'https://b.test microphone deny',
  ])
  assert.deepEqual(permissionRows(kept, 'https://b.test').map((r) => r.kind), ['microphone'])
  assert.equal(rememberedAnswer(kept, { origin: 'https://a.test', kind: 'camera' }), 'allow')
  assert.equal(rememberedAnswer(kept, { origin: 'https://c.test', kind: 'camera' }), null)
  // A private tab never has a remembered answer.
  assert.equal(rememberedAnswer(kept, { origin: 'https://a.test', kind: 'camera', private: true }), null)
})

test('a remembered answer is given at once; otherwise, and always in private, the bar asks', async () => {
  await loadBrowserPrefs(async () => ({ settings: {}, permissions: { 'https://a.test|camera': 'allow' } }))
  setState(() => ({
    tabs: [
      { key: 'n', native: 'tab-1' },
      { key: 'p', native: 'tab-2', private: true },
    ],
    active: 'n',
    prompts: [],
  }))
  await onPermission({ tab: 'tab-1', id: 1, origin: 'https://a.test', kind: 'camera' })
  assert.deepEqual(getState().prompts, [], 'answered from memory, no bar')
  await onPermission({ tab: 'tab-1', id: 2, origin: 'https://a.test', kind: 'microphone' })
  assert.deepEqual(getState().prompts.map((p) => [p.id, p.private]), [[2, false]])
  await onPermission({ tab: 'tab-2', id: 3, origin: 'https://a.test', kind: 'camera' })
  assert.deepEqual(getState().prompts.map((p) => [p.id, p.private]), [[2, false], [3, true]])
  // Answering in private remembers nothing, even when asked to.
  const before = getBrowserPrefs().permissions
  await answerPrompt(getState().prompts[1], true, true)
  assert.equal(getBrowserPrefs().permissions, before)
  assert.deepEqual(getState().prompts.map((p) => p.id), [2])
  // A closed page takes its requests with it.
  dropPromptsFor('tab-1')
  assert.deepEqual(getState().prompts, [])
})

test('forgetting an answer shows at once and comes back when the daemon refuses', async () => {
  await loadBrowserPrefs(async () => ({ settings: {}, permissions: { 'https://a.test|camera': 'deny' } }))
  const sent = []
  await setSitePermission(async (method, path, body) => {
    sent.push([method, path, body])
    return {}
  }, 'https://a.test', 'camera', null)
  assert.deepEqual(sent, [['POST', '/browser/permissions', { origin: 'https://a.test', kind: 'camera', state: null }]])
  assert.deepEqual(getBrowserPrefs().permissions, {})
  const refusal = new Error('nope')
  await assert.rejects(setSitePermission(async () => { throw refusal }, 'https://a.test', 'camera', 'allow'), refusal)
  assert.deepEqual(getBrowserPrefs().permissions, {}, 'put back')
})

test('Settings finds Site permissions, and every surface is wired', () => {
  assert.ok(SETTINGS_ENTRIES.some((e) => e.key === 'browser-permissions' && e.section === 'browser'))
  for (const word of ['site permissions', 'camera', 'microphone']) {
    assert.ok(searchSettings(word, SETTINGS_ENTRIES).some((e) => e.key === 'browser-permissions'), word)
  }
  const view = read('./BrowserView.jsx')
  assert.match(view, /<PermissionBar key=\{`\$\{prompt\.tab\}:\$\{prompt\.id\}`\} prompt=\{prompt\} onAnswer=\{answer\} \/>/)
  assert.match(view, /answerPrompt\(prompt, allow, remember\)\.catch\(\(err\) => notify\(errorCopy\(err\)\)\)/)
  assert.match(view, /permissionRows\(prefs\.permissions, siteOrigin\)/)
  assert.match(view, /dropPromptsFor\(tab\?\.native\)/)
  const bar = read('./BrowserInfobar.jsx')
  assert.match(bar, /className="bw-strip bw-info" role="alert"/)
  assert.match(bar, /!prompt\.private && \(/)
  const store = read('./browser-store.js')
  assert.match(store, /listen\('browser:permission', \(\{ payload \}\) => onPermission\(payload\)\)/)
  const settings = read('./SettingsView.jsx')
  assert.match(settings, /data-stg="browser-permissions"/)
  assert.match(settings, /setSitePermission\(browserCall, p\.origin, p\.kind, null\)\.catch\(\(err\) => setBrowserNote\(errorCopy\(err\)\)\)/)
  assert.match(settings, /keys\.add\('browser-permissions'\)/)
})

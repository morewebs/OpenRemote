import assert from 'node:assert/strict'
import test from 'node:test'
import { chatRights, folderName, menuItems, metaItems, notices, statusLine } from './chat-header.js'

test('folder labels work for both path styles and keep roots meaningful', () => {
  assert.equal(folderName('C:\\dev\\OpenRemote\\'), 'OpenRemote')
  assert.equal(folderName('/home/me/OpenRemote/'), 'OpenRemote')
  assert.equal(folderName('C:\\'), 'C:\\')
  assert.equal(folderName('/'), '/')
  assert.equal(folderName(null), 'Home folder')
})

test('meta keeps the full folder and branch for hover, and lets idle stay quiet', () => {
  const items = metaItems({ workspace: 'C:/dev/OpenRemote', status: 'waiting' }, {
    app: 'Codex', device: 'Studio PC', gitPlace: { branch: 'main', title: 'Branch main' },
  })
  assert.deepEqual(items, [
    { kind: 'folder', label: 'OpenRemote', title: 'C:/dev/OpenRemote' },
    { kind: 'branch', label: 'main', title: 'Branch main' },
    { kind: 'app', label: 'Codex', device: 'Studio PC' },
    { kind: 'status', state: 'waiting', label: 'Needs your decision' },
  ])
  assert.equal(statusLine('idle'), null)
  assert.equal(statusLine('working').label, 'Running')
})

test('menu keeps existing rights without empty separators', () => {
  const rights = (chat, options) => chatRights(chat, { access: 'here', cloudReady: true, canRename: true, ...options })
  const ids = (rights, options) => menuItems(rights, options).map((item) => item.id ?? '|')
  assert.deepEqual(ids(rights({ status: 'running' }), { copyable: true }),
    ['rename', 'copy', 'details', '|', 'sync'])
  assert.deepEqual(ids(rights({ status: 'running', executor: 'elsewhere' }, { access: 'offline' })),
    ['rename', 'copy', 'details', '|', 'delete'])
  assert.deepEqual(ids(rights({ status: 'idle' }, { cloudReady: false })),
    ['rename', 'copy', 'details'])
  assert.deepEqual(ids(rights({ status: 'idle' }, { web: true, canRename: false })), ['copy', 'details'])
  assert.equal(menuItems(rights({ status: 'idle', appAccountId: 'a' }, { canManageAccount: true }), {
    accountDisconnected: true,
  }).find(({ id }) => id === 'account').label, 'Reconnect App account')
})

test('notices keep failed operations visible next to the confirmation', () => {
  const rights = chatRights({ executor: 'another' }, { access: 'not-machine' })
  const rows = notices({ status: 'failed', lastError: 'Stopped unexpectedly' }, rights, {
    confirmation: 'delete', error: 'Try again',
  })
  assert.deepEqual(rows.map(({ kind }) => kind), ['error', 'failure', 'machine', 'delete'])
  assert.equal(rows[0].text, 'Try again')
  assert.equal(rows.at(-1).action, 'Delete everywhere')
  assert.deepEqual(notices({}, chatRights({}, { web: true }), { confirmation: 'delete' }), [])
})

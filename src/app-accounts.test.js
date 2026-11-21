import assert from 'node:assert/strict'
import test from 'node:test'
import { DaemonApi } from './api/daemon.js'
import { accountChoice, accountScope, relayAppAccountSignIn } from './app-accounts.js'
import { chatFromSession } from './state/reducer.js'

test('named accounts require the guarded create route on the selected machine', async () => {
  const requests = []
  const api = new DaemonApi('http://127.0.0.1:9', 'local', '', async (url, options) => {
    requests.push({ url, body: JSON.parse(options.body) })
    return new Response('{}')
  })
  await api.createSession({ harness: 'codex', deviceId: 'machine-b', appAccountId: 'work' })
  await api.createSession({ harness: 'codex', deviceId: 'machine-a' })
  assert.match(requests[0].url, /\/sessions\/with-app-account$/)
  assert.equal(requests[0].body.app_account_id, 'work')
  assert.equal(requests[0].body.device_id, 'machine-b')
  assert.match(requests[1].url, /\/sessions$/)
  assert.equal(Object.hasOwn(requests[1].body, 'app_account_id'), false)
})

test('account mutations and sign-in runs stay scoped to the device, App and account', async () => {
  const requests = []
  const api = new DaemonApi('http://127.0.0.1:9', 'local', '/devices/machine-b', async (url, options) => {
    requests.push({ path: new URL(url).pathname, method: options.method, body: options.body && JSON.parse(options.body) })
    return new Response('{}')
  })
  const base = '/devices/machine-b/harnesses/codex/accounts'
  await api.appAccounts('codex')
  await api.createAppAccount('codex', 'Work')
  await api.updateAppAccount('codex', 'work', { disconnect: true })
  await api.startAppAccountSignIn('codex', 'work')
  await api.appAccountSignInView('codex', 'work', 'first')
  await api.stopAppAccountSignIn('codex', 'work', 'first')
  assert.deepEqual(requests.map(({ path, method }) => [path, method]), [
    [base, 'GET'],
    [base, 'POST'],
    [`${base}/work`, 'PATCH'],
    [`${base}/work/signin`, 'POST'],
    [`${base}/work/signin/first`, 'GET'],
    [`${base}/work/signin/first/stop`, 'POST'],
  ])
  assert.equal(requests[1].body.label, 'Work')
  assert.equal(requests[2].body.disconnect, true)
})

test('a stale account preference needs an explicit replacement', () => {
  const accounts = [
    { id: 'work', label: 'Work', status: 'disconnected' },
    { id: 'personal', label: 'Personal', status: 'ready' },
  ]
  assert.equal(accountChoice(accounts, 'work', true).blocked, true)
  assert.equal(accountChoice(accounts, 'removed', true).blocked, true)
  assert.equal(accountChoice(accounts, 'existing', true).selected.id, 'existing')
  assert.equal(accountChoice(accounts, null, false).selected.id, 'personal')
  assert.notEqual(accountScope('laptop', 'codex'), accountScope('desktop', 'codex'))
  assert.notEqual(accountScope('laptop', 'codex'), accountScope('laptop', 'claude'))
})

test('chat controls can recover the account saved on the executor', () => {
  const session = { id: 'chat', harness: 'codex', executor: 'machine-b', status: 'stopped' }
  assert.equal(chatFromSession({ ...session, app_account_id: 'work' }).appAccountId, 'work')
  assert.equal(chatFromSession(session).appAccountId, null)
})

test('closing during sign-in start stops only the run returned by that start', async () => {
  let finishStart
  const calls = []
  const api = {
    startAppAccountSignIn: () => new Promise((resolve) => { finishStart = resolve }),
    stopAppAccountSignIn: async (...args) => { calls.push(args) },
  }
  const relay = relayAppAccountSignIn(api, 'codex', 'work')
  await Promise.resolve()
  relay.cancel()
  finishStart({ result: { run_id: 'our-run', running: true } })
  await relay.promise
  assert.deepEqual(calls, [['codex', 'work', 'our-run']])
})

test('an effect cancelled before its first turn does not start a login', async () => {
  let starts = 0
  const api = {
    async startAppAccountSignIn() {
      starts += 1
      return { run_id: 'only-run', done: { ok: true } }
    },
  }
  const discarded = relayAppAccountSignIn(api, 'codex', 'work')
  discarded.cancel()
  const active = relayAppAccountSignIn(api, 'codex', 'work')
  await Promise.all([discarded.promise, active.promise])
  assert.equal(starts, 1)
})

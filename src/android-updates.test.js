import assert from 'node:assert/strict'
import test from 'node:test'
import { checkAndroidUpdate, githubAndroidUpdates } from './android-updates.js'

const url = 'https://github.com/morewebs/OpenRemote/releases/download/v0.7.0/OpenRemote-universal.apk'
const release = (tag = 'v0.7.0') => ({ tag_name: tag, draft: false, prerelease: false, assets: [{ name: 'OpenRemote-universal.apk', state: 'uploaded', browser_download_url: url }] })
const fetchRelease = (data) => async () => ({ ok: true, json: async () => data })

test('only native Android GitHub builds get the updater, never Play, web, or desktop', () => {
  assert.equal(githubAndroidUpdates({ native: true, mobile: true, distribution: 'github' }), true)
  for (const options of [{ native: true, mobile: true, distribution: 'play' }, { native: true, mobile: true }, { native: false, mobile: true, distribution: 'github' }, { native: true, mobile: false, distribution: 'github' }]) {
    assert.equal(githubAndroidUpdates(options), false)
  }
})

test('a newer published release offers its official universal APK', async () => {
  assert.deepEqual(await checkAndroidUpdate('0.6.0', { fetcher: fetchRelease(release()) }), { version: '0.7.0', url })
})

test('equal or older releases are up to date, comparing version numbers numerically', async () => {
  for (const current of ['0.7.0', '0.10.0', '1.0.0']) {
    assert.equal(await checkAndroidUpdate(current, { fetcher: fetchRelease(release()) }), null)
  }
  assert.ok(await checkAndroidUpdate('0.7.0-beta.1', { fetcher: fetchRelease(release()) }))
})

test('a failed or malformed check never claims the app is up to date', async () => {
  for (const data of [{}, release('nightly'), { ...release(), prerelease: true }, { ...release(), assets: [] }, { ...release(), assets: [{ ...release().assets[0], browser_download_url: 'https://example.com/app.apk' }] }]) {
    await assert.rejects(checkAndroidUpdate('0.6.0', { fetcher: fetchRelease(data) }))
  }
  await assert.rejects(checkAndroidUpdate('0.6.0', { fetcher: async () => ({ ok: false, status: 403 }) }))
  await assert.rejects(checkAndroidUpdate('0.6.0', { fetcher: async () => { throw new Error('offline') } }))
})

test('the check passes cancellation to the public GitHub request', async () => {
  const signal = new AbortController().signal
  await checkAndroidUpdate('0.7.0', { signal, fetcher: async (endpoint, options) => {
    assert.equal(endpoint, 'https://api.github.com/repos/morewebs/OpenRemote/releases/latest')
    assert.equal(options.signal, signal)
    assert.equal(options.credentials, 'omit')
    return { ok: true, json: async () => release() }
  } })
})

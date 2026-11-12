import assert from 'node:assert/strict'
import test from 'node:test'
import { connectionNotice, detailOf } from './connection-notice.js'

const notice = (extra) => connectionNotice({ state: 'error', restartable: true, ...extra })

test('a working connection says nothing at all', () => {
  assert.equal(connectionNotice({ state: 'connected' }), null)
  assert.equal(connectionNotice({ state: 'connected', everConnected: true }), null)
})

test('the two states that are not a fault yet say nothing', () => {
  assert.equal(connectionNotice({ state: 'connecting' }), null, 'the normal first moment of a session')
  assert.equal(connectionNotice({ state: 'new' }), null, 'no address yet')
  // The waiting screen replaces the whole body and carries the same news and
  // button; a row above it would be the same sentence twice.
  assert.equal(connectionNotice({ state: 'waiting-daemon' }), null)
})

test('a daemon that answered and then stopped says so, with the way to start it again', () => {
  const stopped = notice({ everConnected: true })
  assert.equal(stopped.tone, 'stopped')
  assert.equal(stopped.title, "OpenRemote's background service stopped answering")
  assert.equal(stopped.note, 'It may have exited. Starting it again usually brings the lists back.')
  assert.equal(stopped.action, 'restart')
})

test('a daemon this window never reached has its own words, not the stopped ones', () => {
  const never = notice({ everConnected: false })
  assert.equal(never.tone, 'never')
  assert.equal(never.title, "OpenRemote is not connected to its background service")
  assert.notEqual(never.title, notice({ everConnected: true }).title, 'the two are never the same sentence')
  assert.equal(never.action, 'restart')
})

test('the console argument words are quoted instead of being replaced by ours', () => {
  const refused = notice({ everConnected: false, error: 'unauthorized' })
  assert.equal(refused.note, 'unauthorized')
  const stopped = notice({ everConnected: true, error: 'The daemon stopped answering. ' })
  assert.equal(stopped.note, 'The daemon stopped answering.', 'cleaned of stray whitespace')
})

test('without a restart call the row offers none and tells the person what to do instead', () => {
  const stopped = notice({ everConnected: true, restartable: false })
  assert.equal(stopped.action, null)
  assert.equal(stopped.note, 'It may have exited. Close OpenRemote and open it again.')
  const never = notice({ everConnected: false, restartable: false })
  assert.equal(never.action, null)
  assert.equal(never.note, 'It starts with the app. Open OpenRemote again if it does not appear.')
})

test('a view that already prints the daemon sentence keeps the row away', () => {
  assert.equal(notice({ everConnected: true, showsOwnError: true }), null)
  assert.equal(notice({ everConnected: false, showsOwnError: true }), null)
  // It is the view, not the state, that decides: a stopped daemon is still a
  // stopped daemon, and the view says it where the user is reading.
  assert.equal(notice({ showsOwnError: true, state: 'connected' }), null)
})

test('the row is one line: a long argument is capped on a word boundary', () => {
  const long = 'x'.repeat(400)
  const short = detailOf(long)
  assert.ok(short.length <= 160, 'the cap holds')
  assert.ok(short.endsWith('...'))
  const words = detailOf(`${'word '.repeat(80)}end`)
  assert.ok(words.length <= 160)
  assert.ok(!words.includes('  '), 'no run of spaces survives')
  assert.equal(detailOf('   '), null, 'nothing to quote')
  assert.equal(detailOf(42), null, 'not a sentence')
})

test('the copy is calm: no exclamation marks and no em dash', () => {
  for (const everConnected of [true, false]) {
    for (const restartable of [true, false]) {
      const row = notice({ everConnected, restartable })
      for (const line of [row.title, row.note]) {
        assert.ok(!line.includes('!'), `no exclamation: ${line}`)
        assert.ok(!line.includes('\u2014'), `no em dash: ${line}`)
      }
    }
  }
})

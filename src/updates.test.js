// The once-a-day look for a newer release: when it is due, that it never runs
// twice in a day however often the hourly timer fires, that it waits until the
// app has come up, and that "off" means no look at all. The clock, the timers
// and the storage are handed in, so the day is driven here rather than waited
// out.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { DAY, FIRST_LOOK_AFTER, LOOK_EVERY, isDue, startUpdateChecks } from './updates.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

// A clock and timers the test moves by hand.
function world({ now = 1_000_000_000_000, stored = {} } = {}) {
  const store = new Map(Object.entries(stored))
  const timers = []
  const w = {
    now,
    looks: 0,
    storage: {
      getItem: (k) => (store.has(k) ? store.get(k) : null),
      setItem: (k, v) => store.set(k, String(v)),
    },
    store,
    timers: {
      setTimeout: (fn, ms) => {
        const t = { fn, at: w.now + ms, every: 0 }
        timers.push(t)
        return t
      },
      setInterval: (fn, ms) => {
        const t = { fn, at: w.now + ms, every: ms }
        timers.push(t)
        return t
      },
      clearTimeout: (t) => (t.dead = true),
      clearInterval: (t) => (t.dead = true),
    },
    // Move the clock forward, firing every timer that falls due on the way.
    advance(ms) {
      const end = w.now + ms
      for (;;) {
        const next = timers.filter((t) => !t.dead && t.at <= end).sort((a, b) => a.at - b.at)[0]
        if (!next) break
        w.now = next.at
        if (next.every) next.at += next.every
        else next.dead = true
        next.fn()
      }
      w.now = end
    },
  }
  return w
}

const start = (w, extra = {}) =>
  startUpdateChecks({
    enabled: true,
    timers: w.timers,
    storage: w.storage,
    now: () => w.now,
    check: () => {
      w.looks += 1
    },
    ...extra,
  })

test('a look is due once a day has passed since the last, and not before', () => {
  const now = 10 * DAY
  assert.equal(isDue({ auto: true, last: 0, now }), true, 'never looked')
  assert.equal(isDue({ auto: true, last: NaN, now }), true, 'an unreadable record is no record')
  assert.equal(isDue({ auto: true, last: now - DAY + 1, now }), false, 'a day not yet passed')
  assert.equal(isDue({ auto: true, last: now - DAY, now }), true, 'exactly a day')
  assert.equal(isDue({ auto: false, last: 0, now }), false, 'switched off never looks')
  // A clock set back would otherwise put the last look in the future and stop
  // every look until the clock caught up, possibly years later.
  assert.equal(isDue({ auto: true, last: now + 5 * DAY, now }), true, 'a last look in the future')
})

test('the first look waits for the app to come up, then the day decides', () => {
  const w = world()
  start(w)
  assert.equal(w.looks, 0, 'nothing at start')
  w.advance(FIRST_LOOK_AFTER - 1)
  assert.equal(w.looks, 0, 'not before the start-up delay')
  w.advance(1)
  assert.equal(w.looks, 1, 'one look once the app is up')
})

test('an app left running looks once a day, not once an hour', () => {
  const w = world()
  start(w)
  w.advance(FIRST_LOOK_AFTER)
  assert.equal(w.looks, 1)
  // The hourly timer fires 23 more times inside the day: none of them looks.
  w.advance(DAY - LOOK_EVERY)
  assert.equal(w.looks, 1, 'still one look within the first day')
  w.advance(LOOK_EVERY * 2)
  assert.equal(w.looks, 2, 'a second look once the day has passed')
  w.advance(DAY * 3)
  assert.equal(w.looks, 5, 'one a day after that')
})

test('a restart within the day does not look again', () => {
  const now = 1_000_000_000_000
  const w = world({ now, stored: { 'openremote-last-update-check': String(now - 2 * 3600 * 1000) } })
  start(w)
  w.advance(FIRST_LOOK_AFTER + LOOK_EVERY * 3)
  assert.equal(w.looks, 0, 'looked two hours ago, before the restart')
  w.advance(DAY)
  assert.equal(w.looks, 1, 'the next day does')
})

test('the attempt is recorded, so a failing look still waits a day', () => {
  const w = world()
  start(w, {
    check: () => {
      w.looks += 1
      throw new Error('offline')
    },
  })
  w.advance(FIRST_LOOK_AFTER)
  assert.equal(w.looks, 1)
  assert.equal(Number(w.store.get('openremote-last-update-check')), w.now)
  w.advance(LOOK_EVERY * 5)
  assert.equal(w.looks, 1, 'no hourly retries against a release page that is not answering')
})

test('switched off, it never looks; switched back on, the next tick does', () => {
  const w = world({ stored: { 'openremote-check-updates': 'off' } })
  start(w)
  w.advance(DAY * 2)
  assert.equal(w.looks, 0)
  w.store.set('openremote-check-updates', 'on')
  w.advance(LOOK_EVERY)
  assert.equal(w.looks, 1)
})

test('a phone, a browser tab and a stopped schedule never look', () => {
  const w = world()
  const stop = start(w, { enabled: false })
  w.advance(DAY * 2)
  assert.equal(w.looks, 0, 'not enabled here')
  const w2 = world()
  const stop2 = start(w2)
  stop2()
  w2.advance(DAY * 2)
  assert.equal(w2.looks, 0, 'stopped before the first look')
  assert.equal(typeof stop, 'function')
})

test('storage that throws reads as never looked and on', () => {
  const w = world()
  start(w, {
    storage: {
      getItem: () => {
        throw new Error('blocked')
      },
      setItem: () => {
        throw new Error('blocked')
      },
    },
  })
  w.advance(FIRST_LOOK_AFTER)
  assert.equal(w.looks, 1)
})

test('the schedule starts with the console, and the look sends nothing of its own', () => {
  // main.jsx is where the console boots; the schedule starts there once.
  assert.match(read('./main.jsx'), /startUpdateChecks\(\)/)
  // The console only invokes the shell's two commands: no fetch of its own, no
  // id, no count. What reaches the network is the updater plugin's request.
  const src = read('./updates.js')
  assert.doesNotMatch(src, /\bfetch\(/)
  assert.match(src, /'update_check'/)
  assert.match(src, /'update_install'/)
})

test('the console, the shell and the capability agree on the two commands', () => {
  const build = read('../src-tauri/build.rs')
  const declared = [...build.matchAll(/"([a-z_]+)"/g)].map((m) => m[1])
  for (const command of ['update_check', 'update_install']) {
    assert.ok(declared.includes(command), `${command} is declared to Tauri`)
  }
  const caps = JSON.parse(read('../src-tauri/capabilities/default.json'))
  for (const permission of ['allow-update-check', 'allow-update-install']) {
    assert.ok(caps.permissions.includes(permission), `${permission} is granted to the console`)
  }
  // The plugin's own commands are not granted: the webview reaches updates
  // only through the shell's two, which check the build is switched on first.
  assert.ok(!caps.permissions.some((p) => p.startsWith('updater:')), 'no updater: permission')
})

test('the updater verifies against the config key and reads only this project\'s releases', () => {
  const conf = JSON.parse(read('../src-tauri/tauri.conf.json'))
  const updater = conf.plugins?.updater
  assert.ok(updater, 'plugins.updater is configured')
  assert.equal(typeof updater.pubkey, 'string', 'the public key lives in the config')
  assert.deepEqual(updater.endpoints, [
    'https://github.com/morewebs/OpenRemote/releases/latest/download/latest.json',
  ])
  assert.equal(updater.dangerousInsecureTransportProtocol, undefined, 'https only')
  // No private key, ever: not in the config, not anywhere in the shell.
  assert.doesNotMatch(read('../src-tauri/tauri.conf.json'), /PRIVATE KEY|secret key/i)
})

test('Settings draws the rows, and the search can land on them', () => {
  const row = read('./UpdatesRow.jsx')
  assert.match(row, /data-stg="install-updates"/)
  assert.match(row, /data-stg="install-updates-auto"/)
  assert.match(row, /errorCopy\(/, 'a failed look reads in the console voice')
  const search = read('./settings-search.js')
  assert.match(search, /row\('install-updates', 'this-install',/)
  assert.match(search, /row\('install-updates-auto', 'this-install',/)
  const view = read('./SettingsView.jsx')
  assert.match(view, /<UpdatesRow \/>/)
  // Desktop shell only: a phone and a browser tab have no installer, so the row
  // is not drawn there and the search does not offer it.
  assert.match(view, /\{!isMobile && hasTauri && <UpdatesRow \/>\}/)
  assert.match(view, /if \(isMobile \|\| !hasTauri\) \{\s*keys\.add\('install-updates'\)\s*keys\.add\('install-updates-auto'\)/)
})

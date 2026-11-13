// The look for a newer release: when it is due (every two hours, and a look
// soon after launch), that it never runs twice inside the two hours however
// often the schedule asks, that it waits until the app has come up, that "off"
// means no look at all, and what the top bar and its confirmation say. The
// clock, the timers and the storage are handed in, so the two hours are driven
// here rather than waited out.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import {
  ASK_EVERY,
  FIRST_LOOK_AFTER,
  LOOK_EVERY,
  confirmCopy,
  installCommand,
  isDue,
  startUpdateChecks,
  updateBadge,
} from './updates.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

const HOUR = 3600 * 1000
const DAY = 24 * HOUR
const LAST_KEY = 'openremote-last-update-check'
const AUTO_KEY = 'openremote-check-updates'

// A clock and timers the test moves by hand.
function world({ now = 1_000_000_000_000, stored = {} } = {}) {
  const store = new Map(Object.entries(stored))
  const timers = []
  const w = {
    now,
    looks: 0,
    lookAt: [],
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
      w.lookAt.push(w.now)
    },
    ...extra,
  })

const offer = { version: '0.6.0', notes: null, date: null, page: 'https://example.test/releases/v0.6.0', can_install: true, ready: false }

test('a look is due two hours after the last, and not before', () => {
  const now = 10 * DAY
  assert.equal(isDue({ auto: true, last: 0, now }), true, 'never looked')
  assert.equal(isDue({ auto: true, last: NaN, now }), true, 'an unreadable record is no record')
  assert.equal(isDue({ auto: true, last: now - LOOK_EVERY + 1, now }), false, 'two hours not yet passed')
  assert.equal(isDue({ auto: true, last: now - LOOK_EVERY, now }), true, 'exactly two hours')
  assert.equal(isDue({ auto: true, last: now - HOUR, now }), false, 'an hour ago is not due')
  assert.equal(isDue({ auto: false, last: 0, now }), false, 'switched off never looks')
  // A clock set back would otherwise put the last look in the future and stop
  // every look until the clock caught up, possibly years later.
  assert.equal(isDue({ auto: true, last: now + 5 * DAY, now }), true, 'a last look in the future')
  assert.equal(isDue({ auto: true, last: now + 10 * 60 * 1000, now }), true, 'a few minutes in the future')
})

test('the cadence is two hours, with the first look thirty seconds after start', () => {
  assert.equal(LOOK_EVERY, 2 * HOUR)
  assert.equal(FIRST_LOOK_AFTER, 30_000)
  // Asking is cheap, so the schedule asks often enough that a look never lands
  // more than one ask after its two hours.
  assert.ok(ASK_EVERY > 0 && ASK_EVERY <= 10 * 60 * 1000)
  assert.equal(LOOK_EVERY % ASK_EVERY, 0, 'the asks divide the two hours evenly')
})

test('the first look waits thirty seconds for the app to come up', () => {
  const w = world()
  start(w)
  assert.equal(w.looks, 0, 'nothing at start')
  w.advance(FIRST_LOOK_AFTER - 1)
  assert.equal(w.looks, 0, 'not before the start-up delay')
  w.advance(1)
  assert.equal(w.looks, 1, 'one look once the app is up')
})

test('an app left running looks once every two hours, not on every ask', () => {
  const w = world()
  const launched = w.now
  start(w)
  w.advance(DAY)
  assert.equal(w.lookAt[0], launched + FIRST_LOOK_AFTER, 'the first look at thirty seconds')
  for (let i = 1; i < w.lookAt.length; i++) {
    const gap = w.lookAt[i] - w.lookAt[i - 1]
    assert.ok(gap >= LOOK_EVERY, `look ${i + 1} came ${gap} ms after the one before: under two hours`)
    assert.ok(gap <= LOOK_EVERY + ASK_EVERY, `look ${i + 1} came ${gap} ms after the one before: more than an ask late`)
  }
  assert.equal(w.looks, 12, 'one at thirty seconds, then one every two hours')
})

test('every launch looks once, even within two hours of the last look', () => {
  const now = 1_000_000_000_000
  const w = world({ now, stored: { [LAST_KEY]: String(now - HOUR) } })
  start(w)
  w.advance(FIRST_LOOK_AFTER - 1)
  assert.equal(w.looks, 0, 'not before the start-up delay')
  w.advance(1)
  assert.equal(w.looks, 1, 'a launch looks, whatever the last look was')
  w.advance(HOUR)
  assert.equal(w.looks, 1, 'an hour after that look is still inside the two hours')
  // Due at two hours after the launch look, and asks run every five minutes,
  // so the next ask that counts is the first one at or after that point.
  w.advance(HOUR + ASK_EVERY)
  assert.equal(w.looks, 2, 'the first ask two hours after that look')
})

test('the attempt is recorded, so a failing look still waits two hours', () => {
  const w = world()
  start(w, {
    check: () => {
      w.looks += 1
      w.lookAt.push(w.now)
      throw new Error('offline')
    },
  })
  w.advance(FIRST_LOOK_AFTER)
  assert.equal(w.looks, 1)
  assert.equal(Number(w.store.get(LAST_KEY)), w.now)
  w.advance(LOOK_EVERY - 1)
  assert.equal(w.looks, 1, 'no retries inside two hours against a release page that is not answering')
  w.advance(LOOK_EVERY)
  assert.equal(w.looks, 2, 'the next attempt once two hours have passed')
})

test('switched off, it never looks; switched back on, the next ask does', () => {
  const w = world({ stored: { [AUTO_KEY]: 'off' } })
  start(w)
  w.advance(DAY * 2)
  assert.equal(w.looks, 0)
  w.store.set(AUTO_KEY, 'on')
  w.advance(ASK_EVERY)
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
  // The console only invokes the shell's commands: no fetch of its own, no
  // id, no count. What reaches the network is the updater plugin's request.
  const src = read('./updates.js')
  assert.doesNotMatch(src, /\bfetch\(/)
  assert.match(src, /'update_check'/)
  assert.match(src, /'update_install'/)
  assert.match(src, /'update_open_page'/)
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

test('only an explicit can_install installs; anything else opens the release page', () => {
  assert.equal(installCommand({ ...offer, can_install: true }), 'update_install')
  assert.equal(installCommand({ ...offer, can_install: false }), 'update_open_page', 'Linux is link-only')
  assert.equal(installCommand({ version: '0.6.0' }), 'update_open_page', 'no flag, no install')
  assert.equal(installCommand(null), 'update_open_page', 'nothing offered')
})

test('the badge shows only for an update on a build with updates switched on', () => {
  assert.deepEqual(updateBadge({ available: null, enabled: true, installing: false, runningChats: 0 }), {
    show: false,
    tone: 'blue',
    label: 'Update Available',
    runningChats: 0,
  }, 'no update: nothing shown')
  assert.equal(updateBadge({ available: offer, enabled: false, installing: false, runningChats: 0 }).show, false, 'disabled build: nothing shown')
  assert.equal(updateBadge({ available: offer, enabled: false, runningChats: 3 }).show, false, 'disabled build stays hidden with chats running')
  assert.equal(updateBadge().show, false, 'no arguments: nothing shown')
})

test('running chats make the badge red; an idle computer keeps it blue', () => {
  assert.deepEqual(updateBadge({ available: offer, enabled: true, installing: false, runningChats: 2 }), {
    show: true,
    tone: 'red',
    label: 'Update Available',
    runningChats: 2,
  }, 'running chats: red')
  assert.deepEqual(updateBadge({ available: offer, enabled: true, installing: false, runningChats: 0 }), {
    show: true,
    tone: 'blue',
    label: 'Update Available',
    runningChats: 0,
  }, 'idle: blue')
  assert.equal(updateBadge({ available: offer, enabled: true, runningChats: 1 }).tone, 'red', 'one chat is enough')
  assert.equal(updateBadge({ available: offer, enabled: true, runningChats: -1 }).tone, 'blue', 'a negative count is no chat')
  assert.equal(updateBadge({ available: offer, enabled: true, runningChats: NaN }).runningChats, 0, 'a broken count is no chat')
})

test('the badge hides while an install runs, since the app is about to restart', () => {
  assert.equal(updateBadge({ available: offer, enabled: true, installing: true, runningChats: 0 }).show, false)
})

test('the confirmation for Windows says the app restarts, and warns about running chats', () => {
  assert.deepEqual(confirmCopy({ available: offer, runningChats: 0, canInstall: true }), {
    title: 'OpenRemote 0.6.0 is ready',
    body: ['The app restarts to install it.'],
    warning: null,
    primary: 'Install and restart',
    danger: false,
    secondary: 'Not now',
  })
  const one = confirmCopy({ available: offer, runningChats: 1, canInstall: true })
  assert.equal(one.warning, '1 running chat will stop when the app restarts.')
  assert.equal(one.danger, true, 'the primary button is the danger one')
  assert.equal(one.primary, 'Install and restart')
  const three = confirmCopy({ available: offer, runningChats: 3, canInstall: true })
  assert.equal(three.warning, '3 running chats will stop when the app restarts.')
  assert.equal(three.title, 'OpenRemote 0.6.0 is ready')
})

test('the confirmation for Linux links to the release page, and says when chats stop', () => {
  const idle = confirmCopy({ available: offer, runningChats: 0, canInstall: false })
  assert.equal(idle.title, 'OpenRemote 0.6.0 is on the release page')
  assert.equal(idle.primary, 'Open release page')
  assert.equal(idle.warning, null)
  assert.equal(idle.danger, false)
  assert.equal(idle.body.length, 1)
  // Opening a page stops nothing. The only restart that can stop a chat on
  // Linux is the one the user makes after installing by hand, so the warning
  // names that restart rather than claiming the link stops them.
  const busy = confirmCopy({ available: offer, runningChats: 2, canInstall: false })
  assert.equal(busy.warning, '2 running chats will stop when you restart OpenRemote after installing.')
  assert.equal(busy.danger, true)
  assert.equal(busy.primary, 'Open release page')
  assert.equal(confirmCopy({ available: offer, runningChats: 1, canInstall: false }).warning, '1 running chat will stop when you restart OpenRemote after installing.')
})

test('the copy has no em dashes', () => {
  const copies = [
    confirmCopy({ available: offer, runningChats: 0, canInstall: true }),
    confirmCopy({ available: offer, runningChats: 2, canInstall: true }),
    confirmCopy({ available: offer, runningChats: 0, canInstall: false }),
    confirmCopy({ available: offer, runningChats: 2, canInstall: false }),
  ]
  for (const copy of copies) assert.doesNotMatch(JSON.stringify(copy), /—/)
  assert.doesNotMatch(read('./updates.js'), /—/)
  assert.doesNotMatch(read('./InfoBadge.jsx'), /—/)
  assert.doesNotMatch(read('./info-badge.css'), /—/)
})

test('the badge is the info button in the same place, counting only this computer\'s chats', () => {
  const badge = read('./InfoBadge.jsx')
  // A restart stops the chats this computer runs; a chat on another device is not one.
  assert.match(badge, /stopTargets\(sessions \?\? \[\], \{ thisDevice \}\)\.stoppable\.length/)
  // The info button keeps its name while there is no update.
  assert.match(badge, /aria-label="About OpenRemote"/)
  // The badge's name starts with its visible label.
  assert.match(badge, /aria-label=\{`\$\{badge\.label\}: OpenRemote \$\{version\}`\}/)
  // The confirmation shares the dialog family and its plumbing.
  assert.match(badge, /className="dv-modal-backdrop"/)
  assert.match(badge, /className="dv-modal"/)
  assert.match(badge, /useDialog\(panelRef, onClose\)/)
  assert.match(badge, /useBack\(true, onClose\)/)
  assert.match(badge, /createPortal\(/)
  assert.match(badge, /pn-btn--danger/)
  assert.match(badge, /copy\.secondary/)
  // The title bar draws it where the info button was.
  assert.match(read('./TitleBar.jsx'), /<InfoBadge onAbout=\{\(\) => setAbout\(true\)\} \/>/)
})

test('the badge CSS reuses the palette, and respects reduced motion both ways', () => {
  const css = read('./info-badge.css')
  // No new colours: the tokens only.
  assert.doesNotMatch(css, /#[0-9a-fA-F]{3,8}\b/, 'no hex colours')
  assert.match(css, /var\(--red\)/)
  assert.match(css, /var\(--accent\)/)
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/)
  assert.match(css, /html\[data-reduce-motion\] \.ib-badge/)
})

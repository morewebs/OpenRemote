// The first thirty seconds: three rules that keep the boot from lying,
// flashing, or throwing away what the person typed before the session list
// arrives. The behaviour lives in the provider and the shell, which the test
// runner cannot import (JSX, React state), so these pin the source the way the
// other wiring tests do.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the shell does not dial the address the last run saved', () => {
  // The saved address is the previous daemon's port. Building the client from
  // it made every launch fetch a dead port before the shell had answered, and
  // the refusal was shown as a daemon failure. The client is now built by the
  // one helper that carries the web build's link fetch; the rule stands, so
  // this keeps both halves: the desktop's empty saved address, and the helper
  // staying the only place a client is made.
  const console = source('state/console.jsx')
  assert.match(console, /const saved = hasTauri \? \{\} : loadConnection\(\)/)
  assert.match(console, /return dial\(saved\.url, saved\.token\)/)
  assert.match(console, /const dial = \(url, token, prefix = ''\) => new DaemonApi\(url, token, prefix, webFetch\(\)\)/)
  // And the connect effect has to stay guarded: an empty client never dials.
  assert.match(console, /if \(!api\.ready \|\| connection\.state === 'connected'\) return undefined/)
})

test('a connecting state becomes the waiting screen after a grace, never a verdict', () => {
  const console = source('state/console.jsx')
  // A constant, not a literal buried in the effect.
  const grace = console.match(/const BOOT_GRACE_MS = (\d+)/)
  assert.ok(grace, 'BOOT_GRACE_MS must exist')
  const ms = Number(grace[1])
  // Long enough that a warm start (address in the first daemon_info answer)
  // or a half-second boot never shows the screen; short enough that a person
  // does not sit in front of a wordless app.
  assert.ok(ms >= 2000 && ms <= 4000, `the grace is ${ms}ms`)
  // Only `connecting` may become the waiting screen: a first run is `new` and
  // must keep the wizard. The state may carry extra markers (the waiting
  // screen tells a grace from a verdict), which this rule allows.
  assert.match(
    console,
    /c\.state === 'connecting' \? \{ state: 'waiting-daemon', error: null/,
  )
  // The flip is not a failure: no error is attached, so the waiting screen
  // shows its patient copy rather than "the daemon did not start".
  assert.match(console, /setConnection\(\(c\) =>\s*c\.state === 'connecting'/)
  assert.match(console, /clearTimeout\(slow\)/)
})

test('the New chat composer waits for the boot seed to resolve', () => {
  const app = source('App.jsx')
  // The seed is a chat id the session list cannot validate yet; drawing the
  // live composer there loses whatever was typed when the view moves. The
  // branch carries one more guard today (an error on screen keeps its own
  // words up), which is a refinement rather than a change of the rule.
  assert.match(app, /bootSeed && !sessionsAnswered/)
  assert.match(app, /Opening your chat\.\.\./)
  // The quiet panel is the one the missing-chat pane already uses.
  assert.match(app, /className="cv-none"/)
  // A split pane's New chat is a deliberate one, not a seed: its branch
  // (`paneContent`) must not be guarded by the seed.
  const pane = app.match(/const paneContent = \(leaf\) => \{[\s\S]*?\n  \}/)
  assert.ok(pane, 'paneContent must exist')
  assert.doesNotMatch(pane[0], /bootSeed/)
})

test('the boot is keyed on the connection and its failure, not on the state it shows', () => {
  const console = source('state/console.jsx')
  // A boot chain belongs to a connection. Keying the effect on the state made
  // the grace's own flip cancel an in-flight chain and start a second one, so
  // a daemon that was merely slow was asked twice. The dependency is the
  // failure flag, which only a finished boot can set.
  assert.match(console, /const bootFailed = connection\.state === 'error'/)
  const boot = console.slice(console.indexOf("if (!api.ready || connection.state === 'connected') return undefined"))
  assert.ok(boot.length > 0, 'the boot effect must exist')
  const deps = boot.match(/\}, \[([^\]]*)\]\)/)
  assert.ok(deps, 'the boot effect has a dependency list')
  assert.match(deps[1], /api/)
  assert.match(deps[1], /bootFailed/, 'the boot follows the connection and its failure')
  assert.doesNotMatch(deps[1], /connection\.state/, 'a state change must not restart a running boot')
})

test('a grace with no verdict does not offer the restart, and does not promise it', () => {
  const console = source('state/console.jsx')
  const status = source('DaemonStatus.jsx')
  // The grace marks patience, not failure: nothing said anything went wrong.
  assert.match(console, /state: 'waiting-daemon', error: null, grace: true/)
  // Only the shell's own reason makes the restart button truthful.
  assert.match(status, /connection\?\.grace === true && !daemon\?\.error/)
  assert.match(status, /status\.action === 'try-now' && !booting/)
  // And the note must not promise a button that is not on screen.
  assert.match(status, /\{booting \? BOOTING_NOTE : status\.note\}/)
})
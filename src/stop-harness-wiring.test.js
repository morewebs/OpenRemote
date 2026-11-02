// Stopping one harness's chats, wired: the row only where a harness can
// actually be stopped, the page's two levels, the shared stop loop, and the
// safe answer first. The rules live in stop-harness.test.js; this pins the
// parts only a source read can show.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the palette offers the harness page only when a harness can stop', () => {
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /harnessStops,/)
  assert.match(palette, /const harnessRows = harnessStops\(sessions \?\? \[\], \{ thisDevice \}\)/)
  assert.match(palette, /const harnessRow = harnessRows\.length/)
  assert.match(palette, /name: HARNESS_STOP_NAME/)
  assert.match(palette, /sub: harnessStopSub\(harnessRows\)/)
  assert.match(palette, /run: \{ stopHarnessPage: true \}/)
})

test('the page lists harnesses, asks once, and keeps the safe answer first', () => {
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /if \(page === 'stop-harness'\)/)
  // The pick: one row per harness the rule offers, with its own mark and count.
  assert.match(palette, /everything\.harnessRows\.map\(\(row\) => \(\{/)
  assert.match(palette, /sub: runningLabel\(row\.count\)/)
  assert.match(palette, /<HarnessMark harness=\{row\.id\} size=\{15\} \/>/)
  assert.match(palette, /run: \{ harnessPick: row\.id \}/)
  // The confirm: the harness and its count, named by the rules.
  assert.match(palette, /harnessQuestion\(harnessNameNow, harnessPlan\.stoppable\.length\)/)
  assert.match(palette, /harnessStopping\(harnessNameNow, harnessPlan\.stoppable\.length\)/)
  // Safe answer first, the same way stop-everything orders its two.
  const keep = palette.indexOf("name: 'Keep them running', run: { harnessKeep: true }")
  const stop = palette.indexOf("name: 'Stop them', run: { harnessGo: true }")
  assert.ok(keep > 0 && stop > keep, 'cancel comes before stop')
})

test('it reuses the stop rules: one loop, their names, their aside', () => {
  const palette = source('CommandPalette.jsx')
  // One call site for the stop action, shared by both commands.
  assert.equal((palette.match(/stopChat\(/g) ?? []).length, 1, 'one call site, in the shared loop')
  assert.match(palette, /const stopChats = async \(chats\) => \{/)
  assert.match(palette, /const counts = await stopChats\(plan\.stoppable\)/)
  // The harness's plan is the same stopTargets call over its own sessions.
  assert.match(palette, /stopTargets\(\(sessions \?\? \[\]\)\.filter\(\(s\) => s\.harness === r\.harnessPick\), \{ thisDevice \}\)/)
  // Their sentences, not copies of them.
  assert.match(palette, /elsewhereAside\(harnessPlan\.elsewhere\.length\)/)
  assert.match(palette, /namedChats\(harnessPlan\.stoppable\)/)
  assert.match(palette, /harnessSummary\(\{ name, \.\.\.counts, elsewhere: plan\.elsewhere\.length \}\)/)
})

test('the page is two levels deep, and the keys walk back one level', () => {
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /const pageBack = \(\) => \{/)
  assert.match(palette, /if \(page === 'stop-harness' && harnessStopId\) \{/)
  assert.match(palette, /if \(page\) pageBack\(\)/)
  // Esc and Backspace both call it, and the definition is the third mention.
  assert.equal((palette.match(/pageBack/g) ?? []).length, 3, 'defined once, used twice')
  // Keep them running returns to the harness list, not out of the palette.
  assert.match(palette, /if \(r\.harnessKeep\) \{/)
})

test('the empty case says so on the page instead of showing an empty group', () => {
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /HARNESS_PICK_LINE/)
  assert.match(palette, /HARNESS_EMPTY_LINE/)
  assert.match(palette, /flat\.length === 0 && page !== 'stop-all' && page !== 'stop-harness'/)
})

test('every row the palette had is still offered', () => {
  // Six rows and pages now share this list; a merge must not drop one.
  const palette = source('CommandPalette.jsx')
  for (const name of [
    "name: 'Stop all running chats'",
    'name: HARNESS_STOP_NAME',
    "name: 'Race a prompt across apps'",
    "name: 'Since your last look'",
    "name: 'Changes in this chat'",
    "name: 'Turn this chat into a recurring job'",
  ]) {
    assert.ok(palette.includes(name), `${name} is still offered`)
  }
  const list = palette.slice(palette.indexOf('actions: ['), palette.indexOf('chats: chatRows'))
  for (const group of ['stopRow', 'harnessRow', 'raceRow', 'sinceRow', 'review', 'jobRow']) {
    assert.match(list, new RegExp(`\\.\\.\\.${group}\\b`), `${group} is spread into the list`)
  }
  // The stop lane's pin: stopChat stays last in the console call.
  assert.match(palette, /stopChat \} = useConsole\(\)/)
})

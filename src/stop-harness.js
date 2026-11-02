// Stopping one harness's chats.
//
// Six harnesses run in one window, and the console offered one grouping:
// everything. A person who wants to end the Claude work and keep the Codex
// work had to visit each chat. This module turns the same running set into
// per-harness rows, so the palette can offer one page that stops a single
// harness's chats.
//
// It does not re-implement the stop rules. `stopTargets` already answers
// "which of these sessions can this daemon stop, and which are on another
// computer", for whatever list it is given, so each harness is that call over
// that harness's own sessions. The question, the names under it and the
// skipped-chats aside are the stop-all ones; only the sentences that name a
// harness are new here.
//
// What is offered is what can be done: a harness appears only when at least
// one of its running chats is on this computer. A harness whose running
// chats are all on another computer cannot be stopped from here, and a row
// that would stop nothing is a lie in a list of actions.

import { harnessName } from './harness-names.js'
import { stopSummary, stopTargets } from './stop-everything.js'

/** The palette row that opens the page. */
export const HARNESS_STOP_NAME = "Stop one app's chats"

/** The page's own line, above the list: the scope, said before the press. */
export const HARNESS_PICK_LINE = 'Only chats running on this computer can stop; the rest keep running.'

/** Said on the page when the work it was opened for has ended already. */
export const HARNESS_EMPTY_LINE = 'Nothing is running on this computer any more.'

/** "2 chats" and "1 chat", the noun every sentence here carries. */
function chats(n) {
  return n === 1 ? '1 chat' : `${n} chats`
}

/**
 * The harnesses that can be stopped right now, one row each, biggest first.
 *
 * The order is the count, because the harness spending the most is the one a
 * stop is usually about; ties fall to the console's own name for the harness,
 * so two polls of the same set never reshuffle the page under a cursor.
 */
export function harnessStops(sessions, { thisDevice = null } = {}) {
  const byHarness = new Map()
  for (const session of Array.isArray(sessions) ? sessions : []) {
    const id = session?.harness
    if (!id) continue
    const list = byHarness.get(id) ?? []
    list.push(session)
    byHarness.set(id, list)
  }
  const rows = []
  for (const [id, list] of byHarness) {
    const targets = stopTargets(list, { thisDevice })
    // Nothing this daemon can stop: not offered, whatever runs elsewhere.
    if (targets.stoppable.length === 0) continue
    rows.push({
      id,
      name: harnessName(id),
      count: targets.stoppable.length,
      elsewhere: targets.elsewhere.length,
    })
  }
  return rows.sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
}

/** The root row's quiet sub line: how much work the page would end. */
export function harnessStopSub(rows) {
  const list = rows ?? []
  if (list.length === 0) return ''
  const total = list.reduce((sum, row) => sum + row.count, 0)
  if (list.length === 1) return `${chats(total)} in ${list[0].name}`
  return `${chats(total)} in ${list.length} apps`
}

/** The confirm, naming the harness and the count: the two facts to read. */
export function harnessQuestion(name, count) {
  return `Stop ${chats(count)} in ${name}?`
}

/** Said while the stops are still landing; the outcome follows. */
export function harnessStopping(name, count) {
  return `Stopping ${chats(count)} in ${name}.`
}

/**
 * What happened, with the harness named. The rest of the sentence is
 * stop-everything's own, verbatim: a skipped chat is named rather than
 * hidden, and a chat that is already gone is not a failure. Its first
 * sentence only counts ("Stopped 2."), so this replaces that sentence with
 * the one that says whose work stopped.
 */
export function harnessSummary({ name, stopped = 0, elsewhere = 0, gone = 0, failed = 0 } = {}) {
  const rest = stopSummary({ stopped, elsewhere, gone, failed }).replace(/^[^.]*\.\s*/, '')
  const head = stopped > 0 ? `Stopped ${chats(stopped)} in ${name}.` : `Nothing was stopped in ${name}.`
  return rest ? `${head} ${rest}` : head
}

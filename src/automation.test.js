// The automation vocabulary the views share: the trigger sentence a rule row
// shows, the dirty test that keeps the rule editor from throwing away a
// half-written rule on a stray click, and the list's search.

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  MACHINE_SIGNED_OUT,
  MACHINE_UNKNOWN,
  RULE_EMPTY_NONE,
  RULE_SEARCH_HINT_FROM,
  formDiffers,
  machineNotice,
  matchesRule,
  queryWords,
  ruleCountLine,
  ruleEmptyLine,
  ruleHaystack,
  ruleSearchHint,
  searchRules,
  whenSentence,
  chosenEffort,
} from './automation.js'

// A rule as the daemon stores it, plus the fields the list draws.
const ruleOf = (over = {}) => ({
  id: 'r1',
  name: 'Morning review',
  trigger: { kind: 'schedule', time: '07:30' },
  harness: 'claude',
  machine: 'm1',
  task: 'Review the open pull requests',
  workspace: 'C:\\work\\api',
  enabled: true,
  ...over,
})

test('a rule reads as the sentence a person wrote it in', () => {
  assert.equal(whenSentence({ kind: 'schedule', time: '07:30' }), 'Every day at 07:30')
  assert.equal(whenSentence({ kind: 'webhook' }), 'A webhook arrives')
  // A rule saved without a time still reads as the default it will run at.
  assert.equal(whenSentence({ kind: 'schedule' }), 'Every day at 09:00')
})

test("a rule's effort survives only when the picked model offers it", () => {
  assert.equal(chosenEffort('high', ['low', 'high']), 'high')
  assert.equal(chosenEffort('high', ['low']), undefined)
  assert.equal(chosenEffort(null, ['high']), undefined)
  assert.equal(chosenEffort('high', null), undefined)
})

test('an untouched form is not dirty, whatever the snapshot holds', () => {
  const initial = { name: 'Nightly', task: 'Triage the failing tests', enabled: true }
  assert.equal(formDiffers({ ...initial }, initial), false)
  // No snapshot means nothing to compare, and nothing to lose.
  assert.equal(formDiffers(initial, null), false)
})

test('any changed field makes the form dirty, and a change back clears it', () => {
  const initial = { name: 'Nightly', task: 'Triage the failing tests', enabled: true, machineId: 'm1' }
  assert.equal(formDiffers({ ...initial, name: 'Nightly ' }, initial), true, 'a trailing space is a change')
  assert.equal(formDiffers({ ...initial, task: '' }, initial), true, 'erasing the task counts')
  assert.equal(formDiffers({ ...initial, enabled: false }, initial), true, 'a flipped switch counts')
  assert.equal(formDiffers({ ...initial, machineId: 'm2' }, initial), true, 'a picker pick counts')
  // A field the snapshot never held is the snapshot's business, not the
  // comparison's: adding it there is how a new field joins the guard.
  assert.equal(formDiffers({ ...initial, model: 'opus' }, initial), false)
})

// ---- the list's search ----

test('a query matches the words the row prints: name, trigger and harness', () => {
  const rule = ruleOf()
  assert.equal(matchesRule(rule, 'review'), true, 'the name')
  assert.equal(matchesRule(rule, 'schedule'), false, 'the wire word is not printed')
  assert.equal(matchesRule(rule, 'every day'), true, 'the trigger sentence')
  assert.equal(matchesRule(rule, '07:30'), true, 'the trigger time, because the row shows it')
  assert.equal(matchesRule(rule, 'claude'), true, 'the harness')
  assert.equal(matchesRule(rule, 'Claude Code'), true, 'the harness display name, both words')
  assert.equal(matchesRule(ruleOf({ trigger: { kind: 'webhook' } }), 'webhook'), true)
  assert.equal(matchesRule(ruleOf({ trigger: { kind: 'webhook' } }), 'every day'), false)
})

test('a query does not match what the row does not show', () => {
  const rule = ruleOf()
  // The task, the project and the machine are not printed on the list row.
  assert.equal(matchesRule(rule, 'pull requests'), false)
  assert.equal(matchesRule(rule, 'api'), false)
  assert.equal(matchesRule(rule, 'm1'), false)
})

test('a word cannot match across the boundary between two printed fields', () => {
  // The name and the harness are separate things on the row. Joined into one
  // string they would answer "andu" for the name "Stand" and the harness
  // "Up", a result the reader cannot find anywhere on the row.
  const split = ruleOf({ name: 'Stand', harness: 'up' })
  assert.equal(matchesRule(split, 'andu'), false)
  assert.equal(matchesRule(split, 'stand'), true, 'each field still matches on its own')
  assert.equal(matchesRule(split, 'up'), true)
})

test('the search field arrives with its hint, at the chosen threshold', () => {
  // The field and the hint are one thing in the view: below the threshold
  // there is no field, so the helper answers nothing; from the threshold the
  // field shows the hint. Six is chosen by eye, not measured.
  assert.equal(ruleSearchHint(RULE_SEARCH_HINT_FROM - 1), '')
  assert.equal(ruleSearchHint(0), '')
  assert.equal(ruleSearchHint(RULE_SEARCH_HINT_FROM), 'Search rules')
  assert.equal(ruleSearchHint(RULE_SEARCH_HINT_FROM + 4), 'Search rules')
})

test('case and whitespace do not matter, and an empty query filters nothing', () => {
  const rule = ruleOf()
  assert.equal(matchesRule(rule, '  MORNING  '), true)
  assert.equal(matchesRule(rule, 'claUde'), true)
  // Two words both have to be there, in any field: one query, one search.
  assert.equal(matchesRule(rule, '  morning   claude  '), true)
  assert.equal(matchesRule(rule, 'morning codex'), false)
  for (const empty of ['', '   ', null, undefined]) {
    assert.equal(matchesRule(rule, empty), true)
  }
  assert.deepEqual(queryWords('  Two   Words '), ['two', 'words'])
})

test('the search keeps the order the list already had, and drops only non-matches', () => {
  const rules = [
    ruleOf({ id: 'a', name: 'Morning review' }),
    ruleOf({ id: 'b', name: 'Nightly triage', harness: 'codex' }),
    ruleOf({ id: 'c', name: 'Weekly review' }),
  ]
  assert.deepEqual(searchRules(rules, 'review').map((r) => r.id), ['a', 'c'])
  assert.deepEqual(searchRules(rules, 'codex').map((r) => r.id), ['b'])
  assert.deepEqual(searchRules(rules, '').map((r) => r.id), ['a', 'b', 'c'])
  assert.deepEqual(searchRules(null, 'review'), [])
})

test("a rule's switch is not a word: disabled rules match the same", () => {
  const on = ruleOf({ enabled: true })
  const off = ruleOf({ enabled: false })
  for (const query of ['morning', 'claude', 'every day', '07:30']) {
    assert.equal(matchesRule(on, query), matchesRule(off, query), `${query} matches alike`)
  }
  // And the haystack itself is identical: the state is not in it.
  assert.equal(ruleHaystack(on), ruleHaystack(off))
})

test('the count line says how much of the list is on screen', () => {
  assert.equal(ruleCountLine(3, 12), '3 of 12 rules')
  assert.equal(ruleCountLine(12, 12), '12 of 12 rules')
  assert.equal(ruleCountLine(1, 1), '1 of 1 rule')
})

test('the empty answers are two different sentences', () => {
  // Nothing to filter: the list's own invitation, in the form button's words.
  assert.equal(RULE_EMPTY_NONE, 'No rules yet. Create one.')
  assert.equal(ruleEmptyLine(''), RULE_EMPTY_NONE)
  assert.equal(ruleEmptyLine('   '), RULE_EMPTY_NONE)
  // A query that found nothing names the query, in the console's own quotes.
  assert.equal(ruleEmptyLine('  transcription '), 'No rules match “transcription”.')
})

test('the hint appears when the list is long enough to need one', () => {
  // Below the threshold the list fits in a glance, so the field stays bare.
  for (const total of [0, 1, 5]) assert.equal(ruleSearchHint(total), '')
  assert.equal(RULE_SEARCH_HINT_FROM, 6)
  for (const total of [6, 12, 40]) assert.equal(ruleSearchHint(total), 'Search rules')
  assert.equal(ruleSearchHint(undefined), '')
})

// ---- when no machine can run a rule ----

const machine = (name, status) => ({ name, status })

test('a machine that is on says nothing, whatever else the list holds', () => {
  assert.equal(machineNotice({ answered: true, machines: [machine('here', 'online')], signedIn: true }), null)
  assert.equal(
    machineNotice({ answered: true, machines: [machine('build-box', 'waiting'), machine('here', 'online')], signedIn: false }),
    null,
  )
})

test('a list that has not answered is unknown, not empty, signed out or not', () => {
  // The state this rule exists to keep honest: a console with no machine list
  // yet must not be told it has no machine, and signing out does not change
  // what the daemon knows.
  assert.equal(machineNotice({ answered: false, machines: [], signedIn: true }), null)
  assert.equal(machineNotice({ answered: false, machines: [], signedIn: false }), null)
  assert.equal(machineNotice({ answered: false, machines: [machine('build-box', 'waiting')], signedIn: true }), null)
  assert.equal(machineNotice({}), null)
})

test('a list with no rows says nothing: a running daemon always lists this computer', () => {
  // The absent-machine case cannot happen on a running daemon (store.rs
  // ensures this computer's row on every start), so there is no sentence for
  // it - inventing one would describe a state nobody meets.
  assert.equal(machineNotice({ answered: true, machines: [], signedIn: true }), null)
  assert.equal(machineNotice({ answered: true, machines: [], signedIn: false }), null)
})

test('the machine that has not come online is named, because there is one', () => {
  const one = machineNotice({ answered: true, machines: [machine('build-box', 'waiting')], signedIn: true })
  assert.equal(
    one.text,
    'A rule runs on a machine. build-box has not come online yet, so open Machines when its agent is running.',
  )
  assert.equal(one.action, 'machines')
  // More than one waiting machine: the first is named, the rest are counted.
  const two = machineNotice({
    answered: true,
    machines: [machine('build-box', 'waiting'), machine('mac-mini', 'waiting')],
    signedIn: true,
  })
  assert.match(two.text, /build-box and 1 other machine has not come online yet/)
  // A row with no name still produces a sentence, never "undefined".
  const nameless = machineNotice({ answered: true, machines: [{ status: 'waiting' }], signedIn: true })
  assert.match(nameless.text, /A machine has not come online yet/)
})

test('the machine that answered before and is off now is named too', () => {
  const row = machineNotice({ answered: true, machines: [machine('build-box', 'offline')], signedIn: true })
  assert.match(row.text, /build-box is offline right now/)
  assert.equal(row.action, 'machines')
  // A status this console does not know is still a row, and still no machine.
  assert.equal(
    machineNotice({ answered: true, machines: [machine('odd-box', 'paused')], signedIn: true }).text,
    MACHINE_UNKNOWN,
  )
})

test('signed out, the action is the sign-in door, not a machine row it cannot use', () => {
  const row = machineNotice({ answered: true, machines: [machine('build-box', 'waiting')], signedIn: false })
  assert.equal(row.text, MACHINE_SIGNED_OUT)
  assert.equal(row.action, 'account')
})

test('the line is calm: what a rule waits for first, no exclamation, no em dash', () => {
  const cases = [
    [machine('build-box', 'waiting')],
    [machine('build-box', 'offline')],
    [machine('odd-box', 'paused')],
    [machine('build-box', 'waiting'), machine('mac-mini', 'waiting')],
  ]
  for (const signedIn of [true, false]) {
    for (const machines of cases) {
      const row = machineNotice({ answered: true, machines, signedIn })
      assert.ok(row.text.startsWith('A rule runs on a machine.'), 'the trigger is said first')
      assert.ok(!row.text.includes('!'), row.text)
      assert.ok(!row.text.includes('\u2014'), row.text)
      assert.ok(!/undefined/.test(row.text), row.text)
      assert.ok(['machines', 'account'].includes(row.action))
    }
  }
})
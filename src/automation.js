// Automation vocabulary: the trigger kinds with real event sources, and
// their plain sentences. The connector-backed kinds (pipeline, errors,
// review, release) appear when their connectors do - until then the form
// offers only what can really fire.

import { harnessLabel } from './harness-names.js'

export const KINDS = ['schedule', 'webhook']

// The kinds whose event sources still need connectors.
export const CONNECTOR_KINDS = ['pipeline', 'errors', 'review', 'release']

export function whenSentence(trigger) {
  if (trigger?.kind === 'webhook') return 'A webhook arrives'
  if (trigger?.time) return `Every day at ${trigger.time}`
  if (trigger?.every_minutes) {
    const minutes = trigger.every_minutes
    return `Every ${minutes} minute${minutes === 1 ? '' : 's'}`
  }
  return 'Every day at 09:00'
}

/**
 * The trigger a schedule saves for the cadence the form shows. `daily`
 * keeps the picked HH:MM; a number of minutes is an interval, and the
 * daily time is deliberately absent so the two can never both be stored.
 */
export function scheduleTrigger(cadence, time) {
  if (cadence === 'daily') return { kind: 'schedule', time }
  return { kind: 'schedule', every_minutes: Number(cadence) }
}

/**
 * The effort a rule should save for the model it picked: only a tier that
 * model's own catalog offers. A model switch that leaves the word behind
 * would save a pick the harness cannot run, so it is dropped instead.
 */
export function chosenEffort(effort, tiers) {
  if (!effort) return undefined
  return (Array.isArray(tiers) ? tiers : []).includes(effort) ? effort : undefined
}

// The dirty test lives in form-dirty.js now, because the endpoint editor asks
// the same question and a second copy would drift. Re-exported here so the
// rule editor's import keeps reading as automation's own.
export { formDiffers } from './form-dirty.js'

// ---- the list's search ----
//
// The rules list draws every rule it has, and there is no second place to look
// one up, so search is the list's only way in. It matches what a row prints:
// the rule's name, the sentence its trigger shows (which carries the trigger's
// own kind words and its time) and the harness's display name. The label is the
// row's own, read from the daemon's harness list when the caller passes it, so
// a rule on a plugin's harness prints the name that plugin ships and is found
// by that name too. It does not match what a row does not show - the task text,
// the project, the machine - because a result you cannot see is worse than no
// result, and it follows the same rule the plugins Discover search follows:
// every whitespace-separated word must appear somewhere in what the row shows.
//
// A rule's switch is not a word: a disabled rule is still a rule in the list,
// and searching for its name finds it whatever its state.

/** The only line the list shows when there is nothing to filter yet. The noun
 *  is the contract's ("rule"), and the verb is the one the form's button uses
 *  ("Create rule"), so the empty list and the button that fills it agree. */
export const RULE_EMPTY_NONE = 'No rules yet. Create one.'

// The count at which the list gets its search field, and so its hint: the
// field and the placeholder arrive together. Six is a chosen number, not a
// measurement. A rule row is about one line, six of them is roughly what a
// person takes in without scanning, and below that a search field would be a
// control over a list that already fits at a glance. Change it by eye if the
// panel's height changes; nothing measures it.
export const RULE_SEARCH_HINT_FROM = 6

/** The words the row puts on screen for this rule, one entry per field,
 *  lowercased. Kept apart rather than joined because a query word must not be
 *  able to match across the boundary between two fields. `harnesses` is the
 *  daemon's list, for a harness this build has no name of its own for. */
export function ruleParts(rule, harnesses = null) {
  return [rule?.name, whenSentence(rule?.trigger), harnessLabel(rule?.harness, harnesses)]
    .filter((part) => typeof part === 'string' && part.length > 0)
    .map((part) => part.toLowerCase())
}

/** The words the row puts on screen for this rule, lowercased, as one string
 *  for anything that wants to show what was searched. Matching does not use it:
 *  see matchesRule. */
export function ruleHaystack(rule, harnesses = null) {
  return ruleParts(rule, harnesses).join(' ')
}

/** The query's words, trimmed and lowercased; [] means no filter. */
export function queryWords(query) {
  return String(query ?? '')
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
}

/** Whether one rule shows what the query asks for. Each word is tested against
 *  each field on its own: a joined string would answer "andu" for the name
 *  "Stand" and the harness "Up", a result the row does not visibly contain. */
export function matchesRule(rule, query, harnesses = null) {
  const words = queryWords(query)
  if (!words.length) return true
  const parts = ruleParts(rule, harnesses)
  return words.every((word) => parts.some((part) => part.includes(word)))
}

/** The rules to draw, in the order the list already had them. */
export function searchRules(rules, query, harnesses = null) {
  return (Array.isArray(rules) ? rules : []).filter((rule) => matchesRule(rule, query, harnesses))
}

/** The quiet line under the field while a query is on: "3 of 12 rules". */
export function ruleCountLine(shown, total) {
  return `${shown} of ${total} ${total === 1 ? 'rule' : 'rules'}`
}

/** The answer when a query matches nothing: it names the query. */
export function ruleEmptyLine(query) {
  const words = String(query ?? '').trim()
  return words ? `No rules match “${words}”.` : RULE_EMPTY_NONE
}

/**
 * The field's hint, or '' when the list is short enough that it needs none.
 * `total` is how many rules the list holds, not how many a search shows.
 */
export function ruleSearchHint(total) {
  return (total ?? 0) >= RULE_SEARCH_HINT_FROM ? 'Search rules' : ''
}

// ---- when no rule can run ----
//
// A rule fires on a machine whose agent is online, and GET /machines is the
// only source of that fact (the account's device list answers a different
// question: who may drive this computer from elsewhere, not who runs a rule
// here, so it is deliberately not mixed in - saying "your rules cannot run"
// because no *device* is a machine would be a lie while the daemon would
// happily fire a rule whose machine is this computer).
//
// What the daemon's list can actually say, read from this tree:
//
//   * this computer's row is created `online` on every daemon start and no
//     code path ever changes it (openremote-core/src/store.rs,
//     `ensure_this_machine`);
//   * a machine added by hand is created `waiting`, and nothing in this tree
//     flips it to `online` - the rule engine skips it until its agent checks
//     in (supervisor.rs: "hasn't checked in - the rule waits");
//   * `offline` is defined in the machine model but no code path writes it.
//
// So the row is silent in normal operation: this computer is always on. The
// one state a person can really meet is a list of machines that have not
// checked in yet, and that is the sentence worth writing. A list that has not
// answered is silence too (unknown is not empty, signed out or not), and a
// list with no rows at all cannot happen here, so it invents nothing.
//
// The rules list stays exactly as it was under this line: a saved rule is not
// broken because its machine is asleep, and every rule stays readable and
// editable while the line is up.

/** How every sentence below starts: what a rule waits for. */
const MACHINE_FIRST = 'A rule runs on a machine.'

/** The list has rows whose status is not one this console knows. */
export const MACHINE_UNKNOWN = `${MACHINE_FIRST} None is online right now, so open Machines to see them.`

/** Signed out, and no machine answers: signing in is the way to have one. */
export const MACHINE_SIGNED_OUT = `${MACHINE_FIRST} This computer is not signed in, so sign in to use one, or to make this computer one.`

/** The machine's own name, or a plain stand-in when the row carries none. */
const machineName = (machine) =>
  typeof machine?.name === 'string' && machine.name.trim() ? machine.name.trim() : 'A machine'

/**
 * The line above the rules list, or null when a machine is on (and while the
 * list has not answered, and when it answered with no machines at all).
 *
 * `machines` is the daemon's own list, one `{ name, status }` per machine;
 * `answered` is whether that list has answered at least once. The action
 * names the surface that changes it: 'machines' for the view whose self row
 * makes this computer a machine, 'account' for the sign-in door.
 */
export function machineNotice({ answered = false, machines = [], signedIn = false } = {}) {
  const list = (Array.isArray(machines) ? machines : []).filter((m) => m && typeof m === 'object')
  // A machine is on: nothing to say, whatever else the list holds.
  if (list.some((m) => m.status === 'online')) return null
  // The list has not answered: an empty list right now is silence, not a fact.
  if (!answered) return null
  // No rows at all cannot happen on a running daemon (it always lists this
  // computer); a line for it would be a sentence for a state nobody meets.
  if (list.length === 0) return null
  const action = signedIn ? 'machines' : 'account'
  if (!signedIn) return { text: MACHINE_SIGNED_OUT, action }
  const waiting = list.filter((m) => m.status === 'waiting')
  if (waiting.length > 0) {
    const first = machineName(waiting[0])
    const others = waiting.length - 1
    return {
      text:
        others === 0
          ? `${MACHINE_FIRST} ${first} has not come online yet, so open Machines when its agent is running.`
          : `${MACHINE_FIRST} ${first} and ${others} other ${others === 1 ? 'machine has' : 'machines have'} not come online yet, so open Machines when one of them is on.`,
      action,
    }
  }
  const offline = list.filter((m) => m.status === 'offline')
  if (offline.length > 0) {
    const first = machineName(offline[0])
    const others = offline.length - 1
    return {
      text:
        others === 0
          ? `${MACHINE_FIRST} ${first} is offline right now, so open Machines to see it and start its agent.`
          : `${MACHINE_FIRST} ${first} and ${others} other ${others === 1 ? 'machine is' : 'machines are'} offline right now, so open Machines to see them.`,
      action,
    }
  }
  return { text: MACHINE_UNKNOWN, action }
}


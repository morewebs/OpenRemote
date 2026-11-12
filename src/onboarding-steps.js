// What the first run says about agents.
//
// The wizard's job is connect, pick, start. The pick step had no action: a
// machine with no harness installed reached "You're all set" over a composer
// that could not send, with the only installer three steps away inside a
// picker. This module decides whether the wizard offers that installer
// where the person already is, and owns every word it says about agents, so
// the offer cannot drift from what the console reports.
//
// Which list counts: `capabilities.harnesses` filtered by `available` -
// installed on this machine. A harness switched off in Plugins is still
// installed, so it does not raise an install offer (turning it back on is the
// fix, and that lives in Plugins). A harness the daemon could install but has
// not is not installed, so it does not count either. Nothing is known before
// the daemon answers, and then the wizard claims nothing.

import { SIGNIN_HARNESSES } from './harness-manager.js'
import { harnessName } from './harness-names.js'

/** The harnesses installed on this machine, whatever their switch says. */
export function installedHarnesses(capabilities) {
  return (capabilities?.harnesses ?? []).filter((h) => h && h.available === true)
}

/// Whether the wizard offers the installer, and the one line it shows with
/// the offer. The line names what is missing and what the button does.
///
/// `known` is false before the daemon has answered: there is nothing to say
/// yet, so the step does not exist.
export function installOffer(capabilities) {
  const installed = installedHarnesses(capabilities)
  const known = Boolean(capabilities)
  const offer = known && installed.length === 0
  return {
    known,
    offer,
    installed: installed.length,
    line: offer
      ? 'No App is installed on this computer yet. Install one and it appears in the App picker.'
      : null,
  }
}

/// What one installed harness's row says about its state, in the console's
/// own words: a signed-in agent is ready, an API key stands in for a
/// sign-in, and a harness that needs its owner's login says so.
export function harnessState(row) {
  if (row?.using_api_key && row.signed_in !== true) return 'Using API key'
  if (row?.signed_in === false && SIGNIN_HARNESSES.has(row.id)) return 'Not signed in'
  return 'Ready'
}

/// The ready step's sentence, for the four states a first run can reach it in:
/// capabilities still on their way, an agent that can run a task, agents that
/// need signing in, and no agent at all (the person skipped the install offer, or
/// could not install one yet). None of the four is a dead end.
///
/// `known` is whether this console has heard from its daemon about what is
/// installed. Without it an unanswered poll read as "no agent is installed" on a
/// machine full of them, on the one screen a first run cannot afford to be wrong.
export function readyLine(installed, { known = true } = {}) {
  const rows = installed ?? []
  if (!known) return 'Checking what is installed on this computer.'
  if (rows.length === 0) {
    return 'No App is installed yet. Add one from the App picker whenever you are ready.'
  }
  const usable = rows.filter((row) => harnessState(row) !== 'Not signed in')
  if (usable.length === 0) {
    return rows.length === 1
      ? `${harnessName(rows[0].id)} is installed but not signed in yet. Sign in from the App picker, then start a chat.`
      : 'Your Apps are installed but not signed in yet. Sign in from the App picker, then start a chat.'
  }
  return 'These Apps are ready. Describe the work, pick a folder, and start a chat.'
}

/// The steps the wizard can visit, in order, for the progress dots. The
/// quiet auto-connect beat is not a step (it lasts a moment and has no
/// control), the manual connect form is one, and the install offer is one
/// only while it exists.
export function onboardingSteps({ manual = false, offer = false } = {}) {
  return ['welcome', ...(manual ? ['setup'] : []), ...(offer ? ['agents'] : []), 'ready']
}

/// Where the current step sits among the dots. A step whose entry has gone
/// (the install offer closed itself because something got installed) counts
/// as the last one, so the dots never point past their own row.
export function stepIndex(steps, step) {
  const list = steps ?? []
  const at = list.indexOf(step)
  return at === -1 ? Math.max(0, list.length - 1) : at
}

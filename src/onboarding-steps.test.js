// The first run's offer to install an agent: whether it exists, what it
// says, and where it sits among the wizard's steps. Each rule is pinned
// because each one is a sentence or a dot on the first screen a person sees.

import assert from 'node:assert/strict'
import test from 'node:test'
import {
  harnessState,
  installOffer,
  installedHarnesses,
  onboardingSteps,
  readyLine,
  stepIndex,
} from './onboarding-steps.js'

const harness = (over = {}) => ({
  id: 'claude',
  name: 'Claude Code',
  available: true,
  enabled: true,
  signed_in: true,
  ...over,
})
const caps = (...harnesses) => ({ harnesses })

test('the list that counts is what is installed, whatever the switch says', () => {
  // Installed and switched off in Plugins is still installed: the fix is the
  // switch, not an installer.
  assert.deepEqual(installedHarnesses(caps(harness({ enabled: false }))).map((h) => h.id), ['claude'])
  // Found by the probe but not installed is not installed.
  assert.deepEqual(installedHarnesses(caps(harness({ available: false }))), [])
  assert.deepEqual(installedHarnesses(caps()), [])
  // Nothing known before the daemon answers.
  assert.deepEqual(installedHarnesses(null), [])
})

test('the offer exists only on a known machine with nothing installed', () => {
  // A machine with nothing installed: the one case that offers an installer.
  assert.equal(installOffer(caps(harness({ available: false }))).offer, true)
  assert.equal(installOffer(caps()).offer, true, 'a daemon that answered with no harnesses')
  // Something installed: no step at all.
  assert.equal(installOffer(caps(harness())).offer, false)
  assert.equal(installOffer(caps(harness({ enabled: false }))).offer, false, 'installed and switched off')
  // Nothing known: no offer, and no claim either.
  const unknown = installOffer(null)
  assert.equal(unknown.offer, false)
  assert.equal(unknown.known, false)
  assert.equal(unknown.line, null)
})

test('the offer line names what is missing and what the button does', () => {
  const { line } = installOffer(caps())
  assert.equal(
    line,
    'No agent is installed on this computer yet. Install one and it appears in the app picker.',
  )
  // Nothing is preselected and nothing installs on its own: the line offers
  // the install, it does not promise one.
  assert.match(line, /Install one/)
  // No offer, no line: the step does not exist to show one.
  assert.equal(installOffer(caps(harness())).line, null)
})

test('a harness row says its own state, in the console own words', () => {
  assert.equal(harnessState(harness()), 'Ready')
  assert.equal(harnessState(harness({ signed_in: false })), 'Not signed in')
  assert.equal(harnessState(harness({ signed_in: false, using_api_key: true })), 'Using API key')
  // A key with no sign-in to speak of (the daemon reports no state at all).
  assert.equal(harnessState(harness({ signed_in: null, using_api_key: true })), 'Using API key')
  // A real sign-in wins over a key that is also configured.
  assert.equal(harnessState(harness({ signed_in: true, using_api_key: true })), 'Ready')
  // A harness whose owner has no relayed login is ready as far as the wizard
  // knows, so its row never claims a sign-in it cannot offer.
  assert.equal(harnessState(harness({ id: 'agy', signed_in: false })), 'Ready')
})

test('the ready step has a sentence for each state it can be reached in', () => {
  // A poll that has not answered yet: the machine may hold any number of agents,
  // and saying "no agent is installed" on a first run was the one claim this
  // screen cannot afford to get wrong.
  assert.equal(
    readyLine([], { known: false }),
    'Checking what is installed on this computer.',
  )
  assert.equal(
    readyLine([harness()], { known: false }),
    'Checking what is installed on this computer.',
  )
  // Skipped the offer, or could not install yet: still able to continue.
  assert.equal(
    readyLine([], { known: true }),
    'No agent is installed yet. Add one from the app picker whenever you are ready.',
  )
  // An agent that can start a chat.
  assert.equal(readyLine([harness()]), 'These agents are ready. Describe the work, pick a folder, and start a chat.')
  // Installed but needing the owner's login, singular and plural.
  assert.equal(
    readyLine([harness({ signed_in: false })]),
    'Claude Code is installed but not signed in yet. Sign in from the app picker, then start a chat.',
  )
  assert.equal(
    readyLine([harness({ signed_in: false }), harness({ id: 'codex', name: 'Codex', signed_in: false })]),
    'Your agents are installed but not signed in yet. Sign in from the app picker, then start a chat.',
  )
  // One usable is enough for the ready sentence.
  assert.equal(
    readyLine([harness(), harness({ id: 'codex', signed_in: false })]),
    'These agents are ready. Describe the work, pick a folder, and start a chat.',
  )
  // An API key stands in for a sign-in.
  assert.equal(
    readyLine([harness({ signed_in: false, using_api_key: true })]),
    'These agents are ready. Describe the work, pick a folder, and start a chat.',
  )
  assert.equal(readyLine(null), readyLine([]))
})

test('the steps run welcome, setup when the connection failed, the offer, ready', () => {
  assert.deepEqual(onboardingSteps({}), ['welcome', 'ready'])
  assert.deepEqual(onboardingSteps({ offer: true }), ['welcome', 'agents', 'ready'])
  assert.deepEqual(onboardingSteps({ manual: true }), ['welcome', 'setup', 'ready'])
  assert.deepEqual(onboardingSteps({ manual: true, offer: true }), ['welcome', 'setup', 'agents', 'ready'])
})

test('the dots point at the step on screen, and never past their row', () => {
  const auto = onboardingSteps({ offer: true })
  assert.equal(stepIndex(auto, 'welcome'), 0)
  assert.equal(stepIndex(auto, 'agents'), 1)
  assert.equal(stepIndex(auto, 'ready'), 2)

  const manual = onboardingSteps({ manual: true, offer: true })
  assert.equal(stepIndex(manual, 'setup'), 1)
  assert.equal(stepIndex(manual, 'agents'), 2)
  assert.equal(stepIndex(manual, 'ready'), 3)

  // The offer closed itself because an install landed: the step is gone, and
  // the dots fall back to the last row rather than pointing at nothing.
  assert.equal(stepIndex(auto, 'setup'), auto.length - 1)
  assert.equal(stepIndex([], 'ready'), 0)
})

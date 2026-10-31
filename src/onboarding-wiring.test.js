// The first run's install offer, wired: the step exists only where the rule
// says it does, it opens the one installer the console already has and never
// installs anything itself, and the way past it is a plain button.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the offer is computed from the console, and the step shows only while it exists', () => {
  const view = source('Onboarding.jsx')
  assert.match(view, /import \{\n?[\s\S]{0,200}installOffer,/)
  assert.match(view, /const offer = installOffer\(capabilities\)/)
  assert.match(view, /const installed = installedHarnesses\(capabilities\)/)
  assert.match(view, /\{step === 'agents' && \(/)
  assert.match(view, /\{offer\.line\}/)
})

test('it opens the harness manager where the person is, and installs nothing itself', () => {
  const view = source('Onboarding.jsx')
  assert.match(view, /import HarnessManagerModal from '\.\/HarnessManagerModal\.jsx'/)
  assert.match(view, /\{managing && <HarnessManagerModal onClose=\{\(\) => setManaging\(false\)\} \/>\}/)
  assert.match(view, /onClick=\{\(\) => setManaging\(true\)\}/)
  // No second installer, no route: the wizard only opens the surface.
  assert.doesNotMatch(view, /installHarness/)
  assert.doesNotMatch(view, /api\./)
})

test('skipping is a button, not a wall: it lands on the ready screen', () => {
  const view = source('Onboarding.jsx')
  assert.match(view, />\s*Skip for now\s*</)
  assert.match(view, /onClick=\{\(\) => setStep\('ready'\)\}/)
  assert.match(view, /Install an agent/)
})

test('the step runs after the connection and before the ready screen', () => {
  const view = source('Onboarding.jsx')
  // The daemon's answer is what moves the wizard on, and it moves it to the
  // offer only when there is one.
  assert.match(view, /const afterSetup = \(\) => \(offer\.offer \? 'agents' : 'ready'\)/)
  assert.match(view, /setStep\(afterSetup\(\)\)/)
  // The offer stands down by itself once an install lands.
  assert.match(view, /if \(step === 'agents' && offer\.known && !offer\.offer\) setStep\('ready'\)/)
  // The dots follow the steps the rule returns.
  assert.match(view, /const steps = onboardingSteps\(\{ manual, offer: offer\.offer \}\)/)
  assert.match(view, /const dotOn = stepIndex\(steps, step\)/)
})

test('the ready screen speaks from the rules, and the old screens still work', () => {
  const view = source('Onboarding.jsx')
  // The sentence comes from the rule, and the rule is told whether the daemon has
  // answered: an unanswered poll used to read as "no agent is installed".
  assert.match(view, /<p className="ob-sub">\{readyLine\(installed, \{ known: capabilities != null \}\)\}<\/p>/)
  assert.match(view, /harnessState\(h\)/)
  // An agent installed but not signed in does not wear the ready green.
  assert.match(view, /ob-harness--quiet/)
  // The welcome, the quiet beat and the manual form are untouched.
  assert.match(view, /One window for all your coding agents/)
  assert.match(view, /Setting up this machine/)
  assert.match(view, /Connect to the daemon/)
  assert.match(view, /onClick=\{onDismiss\}/)
})

test('the quiet row style exists for the states that are not ready', () => {
  const css = source('onboarding.css')
  assert.match(css, /\.ob-harness--quiet \{/)
  assert.match(css, /\.ob-harness--quiet \.ob-harness-state \{/)
})

test('the welcome action is outlined so it reads as a button', () => {
  const view = source('Onboarding.jsx')
  assert.match(view, /className="ob-cta ob-cta--outline"/)
  const css = source('onboarding.css')
  assert.match(css, /\.ob-cta--outline \{[^}]*border: 1px solid/)
})

test('the ready screen can step back to the page before it', () => {
  const view = source('Onboarding.jsx')
  // Back walks the dots' own list backwards from the ready screen, and the
  // quiet setup beat is not a target: it has no control and would only send
  // a connected console forward again.
  assert.match(view, /const backStep = /)
  assert.match(view, /steps\.indexOf\('ready'\) - 1/)
  assert.match(view, /steps\[i\] !== 'setup'/)
  assert.match(view, /onClick=\{\(\) => setStep\(backStep\)\}/)
  // The control sits at the top of the ready panel, above its title.
  const readyAt = view.indexOf("{step === 'ready' && (")
  const backAt = view.indexOf('className="ob-back"')
  const titleAt = view.indexOf("You&apos;re all set")
  assert.ok(readyAt !== -1 && backAt > readyAt && titleAt > backAt)
  // The dots stay alone in the footer's centered column.
  const css = source('onboarding.css')
  assert.match(css, /\.ob-footer \{/)
  assert.match(css, /\.ob-back \{[^}]*align-self: flex-start/)
  assert.match(css, /\.ob-progress \{[^}]*grid-column: 2/)
})

test('the step body fades in, and both motion switches turn it off', () => {
  const view = source('Onboarding.jsx')
  assert.match(view, /className=\{`ob-stage/, 'the stage is the frame the fade runs on')
  assert.match(view, /key=\{step\}/, 'and it remounts per step, which restarts the fade')
  const css = source('onboarding.css')
  assert.match(css, /@keyframes ob-step-in/)
  // The contract asks for both switches, not one: the media query for the OS
  // setting and the in-app toggle, which cannot be expressed as a media query.
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.ob-stage[^{]*\{\s*animation: none;/)
  assert.match(css, /html\[data-reduce-motion\] \.ob-stage/)
})

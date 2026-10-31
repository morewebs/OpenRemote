import { useEffect, useState } from 'react'
import { CaretLeft } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { loadConnection } from './api/daemon.js'
import HarnessManagerModal from './HarnessManagerModal.jsx'
import { harnessName } from './harness-names.js'
import { HarnessMark } from './brand-marks.jsx'
import {
  harnessState,
  installOffer,
  installedHarnesses,
  onboardingSteps,
  readyLine,
  stepIndex,
} from './onboarding-steps.js'
import './onboarding.css'

// Every harness the daemon can drive, in their own spellings - the
// welcome names them all rather than one.
const ALL_HARNESSES = new Intl.ListFormat('en', { type: 'conjunction' }).format(
  ['claude', 'codex', 'grok', 'pi', 'opencode', 'agy'].map(harnessName),
)

// The connect form's starting address. A saved one is the right answer (this console has
// talked to that daemon before); otherwise the placeholder describes the shape, never a
// literal port. It used to read http://127.0.0.1:5175, which is the browser dev server's
// own port: a person following it in the one fallback shown when nothing answered
// connected to the console's dev server and was told it could not be reached.
const savedAddress = () => {
  try {
    return loadConnection()?.url ?? ''
  } catch {
    return ''
  }
}
const ADDRESS_SHAPE = 'http://127.0.0.1:<the port the daemon printed>'
const CONNECT_HINT = 'The daemon prints its address when it starts.'

// The happy path is the welcome, then the ready screen, and the user never
// sees connection plumbing: the daemon comes up with the app. A quiet setup
// beat shows only while it boots (a moment), and the manual connect form is
// a fallback for when auto-connection genuinely fails (browser mode, a
// daemon that never answered). A machine with no harness installed gets one
// more step between them: the offer to install an agent, which opens the
// harness manager on top of the wizard and comes straight back. It is an
// offer, not a gate - Skip for now lands on the ready screen, which says how
// to add one later. `startManual` opens straight on the connect form, for the
// waiting screen's "connect by address" link, and `onDismiss` closes it again
// for someone who opened it to look.
export default function Onboarding({ onDone, onDismiss = null, startManual = false }) {
  const { connection, capabilities, connect } = useConsole()
  const [step, setStep] = useState(startManual ? 'setup' : 'welcome') // welcome | setup | agents | ready
  const [manual, setManual] = useState(startManual)
  const [url, setUrl] = useState(savedAddress)
  const [token, setToken] = useState('')
  // The harness manager, opened where the person already is. It installs
  // through its own chain; the wizard only opens it and takes the answer.
  const [managing, setManaging] = useState(false)

  const offer = installOffer(capabilities)
  const installed = installedHarnesses(capabilities)
  // The step after the connection: the install offer when the machine has no
  // harness at all, otherwise the ready screen. One function, so the button
  // and the quiet beat that follows the daemon's answer cannot disagree.
  const afterSetup = () => (offer.offer ? 'agents' : 'ready')

  const begin = () => {
    if (connection.state === 'connected') setStep(afterSetup())
    else setStep('setup')
  }

  // The quiet setup beat: advance the moment the daemon answers; offer
  // the manual form only if it truly can't (an error, or a slow boot).
  useEffect(() => {
    if (step !== 'setup') return
    if (connection.state === 'connected') {
      setStep(afterSetup())
      return
    }
    if (connection.state === 'error') {
      setManual(true)
      return
    }
    const slowBoot = setTimeout(() => setManual(true), 15000)
    return () => clearTimeout(slowBoot)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, connection.state, offer.offer])

  // The offer stands down by itself once something is installed - the
  // manager installs, the poll reports it, and the step that offered it has
  // nothing left to offer.
  useEffect(() => {
    if (step === 'agents' && offer.known && !offer.offer) setStep('ready')
  }, [step, offer.known, offer.offer])

  const steps = onboardingSteps({ manual, offer: offer.offer })
  const dotOn = stepIndex(steps, step)

  // The ready screen is the wizard's last page, so back means the page
  // before it in the dots' own list: the install offer while the person
  // still has one to visit, otherwise the welcome. The quiet setup beat is
  // never a target - it has no control, and a connected console would land
  // on it only to be sent straight forward again.
  const backStep = (() => {
    for (let i = steps.indexOf('ready') - 1; i >= 0; i -= 1) {
      if (steps[i] !== 'setup') return steps[i]
    }
    return null
  })()

  return (
    <div className="ob">
      <div className="ob-panel">
        <div className={`ob-stage${step === 'welcome' ? ' is-welcome' : ''}`} key={step}>
        {step === 'welcome' && (
          <>
            <h1 className="ob-title">One window for all your coding agents</h1>
            <p className="ob-sub">
              Run {ALL_HARNESSES} side by side. Start tasks, follow every session as it
              runs, approve what the agents do, and put routine work on a schedule.
            </p>
            <button className="ob-cta ob-cta--outline" onClick={begin}>
              Get started
            </button>
          </>
        )}

        {step === 'setup' && !manual && (
          <>
            <h1 className="ob-title">Setting up this machine</h1>
            <p className="ob-sub">This only takes a moment.</p>
          </>
        )}

        {step === 'setup' && manual && (
          <>
            <h1 className="ob-title">Connect to the daemon</h1>
            <p className="ob-sub">
              The daemon on this machine didn&apos;t answer. Connect by address if you run it
              yourself, or reopen the app to try again.
            </p>
            <div className="ob-harnesses">
              <input
                className="ob-input"
                placeholder={ADDRESS_SHAPE}
                title={CONNECT_HINT}
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                spellCheck={false}
              />
              <input
                className="ob-input"
                placeholder="Token"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                spellCheck={false}
              />
              <button
                className="ob-harness ob-cta ob-cta--primary"
                onClick={() => connect(url.trim(), token.trim())}
                disabled={!url.trim() || !token.trim()}
              >
                Connect
              </button>
              {connection.state === 'error' && (
                <p className="ob-sub">Could not reach it: {connection.error}</p>
              )}
              {onDismiss && (
                <button type="button" className="ob-cta" onClick={onDismiss}>
                  Back
                </button>
              )}
            </div>
          </>
        )}

        {step === 'agents' && (
          <>
            <h1 className="ob-title">Add an agent</h1>
            <p className="ob-sub">{offer.line}</p>
            <div className="ob-harnesses">
              <button
                type="button"
                className="ob-cta ob-cta--primary"
                onClick={() => setManaging(true)}
              >
                Install an agent
              </button>
              <button type="button" className="ob-cta" onClick={() => setStep('ready')}>
                Skip for now
              </button>
            </div>
          </>
        )}

        {step === 'ready' && (
          <>
            {backStep && (
              <button type="button" className="ob-back" onClick={() => setStep(backStep)}>
                <CaretLeft size={14} weight="bold" />
                Back
              </button>
            )}
            <h1 className="ob-title">You&apos;re all set</h1>
            <p className="ob-sub">{readyLine(installed, { known: capabilities != null })}</p>
            <div className="ob-harnesses">
              {installed.map((h) => {
                const state = harnessState(h)
                return (
                  <div
                    className={`ob-harness${state === 'Ready' ? ' signed' : ' ob-harness--quiet'}`}
                    key={h.id}
                  >
                    <HarnessMark harness={h.id} size={16} />
                    <span className="ob-harness-name">{harnessName(h.id)}</span>
                    <span className="ob-harness-state">{state}</span>
                  </div>
                )
              })}
            </div>
            <button className="ob-cta ob-cta--primary" onClick={onDone}>
              Open OpenRemote
            </button>
          </>
        )}
        </div>

        <div className="ob-footer">
          <div className="ob-progress">
            {Array.from({ length: steps.length }, (_, i) => (
              <span key={i} className={`ob-dot${i <= dotOn ? ' on' : ''}`} />
            ))}
          </div>
        </div>
      </div>
      {managing && <HarnessManagerModal onClose={() => setManaging(false)} />}
    </div>
  )
}

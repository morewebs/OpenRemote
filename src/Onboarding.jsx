import { useState } from 'react'
import { ArrowRight } from '@phosphor-icons/react'
import { HARNESSES } from './catalog.js'
import './onboarding.css'

const STEPS = ['welcome', 'devices', 'harnesses', 'ready']
const INSTALL = 'curl -fsSL openremote.space/install | sh'

export default function Onboarding({ onDone, onSignIn, harnesses }) {
  const [step, setStep] = useState('welcome')
  const list = harnesses ?? HARNESSES

  const next = () => {
    const i = STEPS.indexOf(step)
    if (i < STEPS.length - 1) setStep(STEPS[i + 1])
    else onDone()
  }

  return (
    <div className="ob">
      <div className="ob-panel">
        {step === 'welcome' && (
          <>
            <h1 className="ob-title">One window for the agents on your machines</h1>
            <p className="ob-sub">
              Claude Code, Codex, OpenCode, and the rest stay installed where they are.
              OpenRemote is where you start a task, read the session, and approve what they run.
            </p>
            <button className="ob-cta" onClick={next}>
              Get started
            </button>
          </>
        )}

        {step === 'devices' && (
          <>
            <h1 className="ob-title">Work starts on this computer</h1>
            <p className="ob-sub">
              Another machine is optional. Run this on it, then add it from Devices.
              It stays waiting until its agent checks in.
            </p>
            <div className="ob-code">{INSTALL}</div>
            <div className="ob-actions">
              <button className="ob-cta ob-cta--primary" onClick={next}>
                Use this computer
              </button>
            </div>
          </>
        )}

        {step === 'harnesses' && (
          <>
            <h1 className="ob-title">Sign in to the harnesses you use</h1>
            <p className="ob-sub">
              Signing in is not installing. A signed-in harness can be put on a machine later, from that machine.
            </p>
            <div className="ob-harnesses">
              {list.slice(0, 6).map((h) => {
                const signed = h.group === 'Signed in'
                return (
                  <button
                    key={h.id}
                    className={`ob-harness${signed ? ' signed' : ''}`}
                    onClick={() => {
                      if (!signed) onSignIn(h.id)
                    }}
                  >
                    <span className="ob-harness-mark">
                      {h.brand ? (
                        <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor">
                          <path d={h.brand.path} />
                        </svg>
                      ) : (
                        <h.icon size={15} weight="light" />
                      )}
                    </span>
                    <span className="ob-harness-name">{h.name}</span>
                    <span className="ob-harness-state">{signed ? 'Signed in' : 'Sign in'}</span>
                  </button>
                )
              })}
            </div>
            <button className="ob-cta ob-cta--primary" onClick={next}>
              Continue
              <ArrowRight size={13} weight="bold" />
            </button>
          </>
        )}

        {step === 'ready' && (
          <>
            <h1 className="ob-title">New tasks run on this computer</h1>
            <p className="ob-sub">
              Describe the work, and pick the project it belongs to. Choose Cloud in the sidebar when a task should run on another machine.
            </p>
            <button className="ob-cta ob-cta--primary" onClick={onDone}>
              Open OpenRemote
            </button>
          </>
        )}

        <div className="ob-progress">
          <span className={`ob-dot${step === 'welcome' ? ' on' : ''}`} />
          <span className={`ob-dot${step === 'devices' ? ' on' : ''}`} />
          <span className={`ob-dot${step === 'harnesses' ? ' on' : ''}`} />
          <span className={`ob-dot${step === 'ready' ? ' on' : ''}`} />
        </div>
      </div>
    </div>
  )
}

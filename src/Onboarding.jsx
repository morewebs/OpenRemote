import { useState } from 'react'
import { ArrowRight, Asterisk } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import './onboarding.css'

const STEPS = ['welcome', 'connect', 'ready']

export default function Onboarding({ onDone }) {
  const { connection, connect, capabilities } = useConsole()
  const [step, setStep] = useState('welcome')
  const [url, setUrl] = useState('')
  const [token, setToken] = useState('')

  const next = () => {
    const i = STEPS.indexOf(step)
    if (i < STEPS.length - 1) setStep(STEPS[i + 1])
    else onDone()
  }

  const harness = capabilities?.harnesses?.find((h) => h.id === 'claude')
  const harnessReady = Boolean(harness?.available)

  return (
    <div className="ob">
      <div className="ob-panel">
        {step === 'welcome' && (
          <>
            <h1 className="ob-title">One window for the agents on your machines</h1>
            <p className="ob-sub">
              Claude Code stays installed where it is. OpenRemote is where you start a
              task, read the session, and approve what it runs.
            </p>
            <button className="ob-cta" onClick={next}>
              Get started
            </button>
          </>
        )}

        {step === 'connect' && (
          <>
            <h1 className="ob-title">Connect to the daemon</h1>
            <p className="ob-sub">
              {connection.state === 'connected'
                ? 'The daemon on this machine answered. Its address came from the app.'
                : 'In the app the daemon starts on its own. Connect by address if you run it yourself.'}
            </p>
            {connection.state !== 'connected' && (
              <div className="ob-harnesses">
                <input
                  className="ob-input"
                  placeholder="http://127.0.0.1:5175"
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
              </div>
            )}
            {connection.state === 'connected' && (
              <button className="ob-cta ob-cta--primary" onClick={next}>
                Continue
                <ArrowRight size={13} weight="bold" />
              </button>
            )}
          </>
        )}

        {step === 'ready' && (
          <>
            <h1 className="ob-title">New tasks run on this computer</h1>
            <p className="ob-sub">
              {harnessReady
                ? 'Claude Code is installed and ready. Describe the work and pick the folder it runs in.'
                : 'No Claude Code CLI was found on this machine — install it, then start a task.'}
            </p>
            <div className="ob-harnesses">
              <div className={`ob-harness${harnessReady ? ' signed' : ''}`}>
                <span className="ob-harness-mark">
                  <Asterisk size={15} weight="light" />
                </span>
                <span className="ob-harness-name">Claude Code</span>
                <span className="ob-harness-state">
                  {harnessReady ? harness.version ?? 'Ready' : 'Not found'}
                </span>
              </div>
            </div>
            <button className="ob-cta ob-cta--primary" onClick={next}>
              Open OpenRemote
            </button>
          </>
        )}

        <div className="ob-progress">
          <span className={`ob-dot${step === 'welcome' ? ' on' : ''}`} />
          <span className={`ob-dot${step === 'connect' ? ' on' : ''}`} />
          <span className={`ob-dot${step === 'ready' ? ' on' : ''}`} />
        </div>
      </div>
    </div>
  )
}

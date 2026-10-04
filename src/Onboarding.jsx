import { useEffect, useState } from 'react'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import './onboarding.css'

// The happy path is two steps - welcome, ready - and the user never sees
// connection plumbing: the daemon comes up with the app. A quiet setup
// beat shows only while it boots (a moment), and the manual connect form
// is a fallback for when auto-connection genuinely fails (browser mode,
// a daemon that never answered).
export default function Onboarding({ onDone }) {
  const { connection, capabilities, connect } = useConsole()
  const [step, setStep] = useState('welcome') // welcome | setup | ready
  const [manual, setManual] = useState(false)
  const [url, setUrl] = useState('')
  const [token, setToken] = useState('')

  const available = (capabilities?.harnesses ?? []).filter((h) => h.available)

  const begin = () => {
    if (connection.state === 'connected') setStep('ready')
    else setStep('setup')
  }

  // The quiet setup beat: advance the moment the daemon answers; offer
  // the manual form only if it truly can't (an error, or a slow boot).
  useEffect(() => {
    if (step !== 'setup') return
    if (connection.state === 'connected') {
      setStep('ready')
      return
    }
    if (connection.state === 'error') {
      setManual(true)
      return
    }
    const slowBoot = setTimeout(() => setManual(true), 15000)
    return () => clearTimeout(slowBoot)
  }, [step, connection.state])

  const dots = manual ? 3 : 2
  const dotOn = step === 'welcome' ? 0 : manual ? 1 : dots - 1

  return (
    <div className="ob">
      <div className="ob-panel">
        {step === 'welcome' && (
          <>
            <h1 className="ob-title">One window for the agents on this computer</h1>
            <p className="ob-sub">
              Claude Code stays installed where it is. OpenRemote is where you start a
              task, read the session, and approve what it runs.
            </p>
            <button className="ob-cta" onClick={begin}>
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
              The daemon on this machine didn't answer. Connect by address if you run it
              yourself, or reopen the app to try again.
            </p>
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
          </>
        )}

        {step === 'ready' && (
          <>
            <h1 className="ob-title">New tasks run on this computer</h1>
            <p className="ob-sub">
              {available.length > 0
                ? 'These harnesses are installed and ready. Describe the work and pick the folder it runs in.'
                : 'No harness CLI was found on this machine - install Claude Code or Codex, then start a task.'}
            </p>
            <div className="ob-harnesses">
              {available.map((h) => (
                <div className="ob-harness signed" key={h.id}>
                  <span className="ob-harness-name">{harnessName(h.id)}</span>
                  <span className="ob-harness-state">Ready</span>
                </div>
              ))}
            </div>
            <button className="ob-cta ob-cta--primary" onClick={onDone}>
              Open OpenRemote
            </button>
          </>
        )}

        <div className="ob-progress">
          {Array.from({ length: dots }, (_, i) => (
            <span key={i} className={`ob-dot${i <= dotOn ? ' on' : ''}`} />
          ))}
        </div>
      </div>
    </div>
  )
}

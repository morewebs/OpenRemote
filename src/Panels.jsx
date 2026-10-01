import { useState } from 'react'
import AboutModal from './AboutModal.jsx'
import { useConsole } from './state/console.jsx'
import './devices.css'
import './panels.css'

export default function Panels({ onReplay }) {
  const { connection, capabilities, sessions, disconnect } = useConsole()
  const [about, setAbout] = useState(false)
  const [reduceMotion, setReduceMotion] = useState(() => {
    try {
      return localStorage.getItem('openremote-reduce-motion') === '1'
    } catch {
      return false
    }
  })
  const [confirmDisconnect, setConfirmDisconnect] = useState(false)
  const harness = capabilities?.harnesses?.find((h) => h.id === 'claude')

  const setReduce = (value) => {
    setReduceMotion(value)
    try {
      localStorage.setItem('openremote-reduce-motion', value ? '1' : '0')
    } catch {
      /* storage unavailable */
    }
    document.documentElement.toggleAttribute('data-reduce-motion', value)
  }

  return (
    <div className="panel">
      <header className="dv-head">
        <h1 className="dv-title">Settings</h1>
        <p className="dv-meta">The daemon connection, the harness it drives, and this install.</p>
      </header>

      <h2 className="pn-section">Daemon</h2>
      <p className="pn-lead">The local process the chats run through.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">{connection.state === 'connected' ? 'Connected' : 'Not connected'}</div>
            <div className="pn-detail">{connection.state === 'connected' ? 'Bearer token accepted.' : connection.error ?? 'Start the app or connect from the first-run steps.'}</div>
          </div>
          {confirmDisconnect ? (
            <div className="pn-actions">
              <button type="button" className="pn-btn" onClick={disconnect}>Disconnect</button>
              <button type="button" className="pn-btn" onClick={() => setConfirmDisconnect(false)}>Cancel</button>
            </div>
          ) : (
            <button
              type="button"
              className="pn-btn"
              onClick={() => setConfirmDisconnect(true)}
              disabled={connection.state !== 'connected'}
            >
              {connection.state === 'connected' ? 'Forget' : '—'}
            </button>
          )}
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">{(sessions ?? []).length} {(sessions ?? []).length === 1 ? 'chat' : 'chats'} on record</div>
            <div className="pn-detail">Sessions survive daemon restarts; stopped ones can resume.</div>
          </div>
        </div>
      </div>

      <h2 className="pn-section">Harness</h2>
      <p className="pn-lead">What the daemon drives on this machine.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">Claude Code</div>
            <div className="pn-detail">
              {harness?.available ? harness.path : 'No CLI found. Install Claude Code and reopen the app.'}
            </div>
          </div>
          <span className={`pn-btn${harness?.available ? ' on' : ''}`}>{harness?.available ? 'Ready' : 'Missing'}</span>
        </div>
      </div>

      <h2 className="pn-section">This install</h2>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">OpenRemote 0.1.0</div>
            <div className="pn-detail">Daemon {capabilities?.daemon ?? '—'} · slice 1</div>
          </div>
          <button type="button" className="pn-btn" onClick={() => setAbout(true)}>About</button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Reduce motion</div>
            <div className="pn-detail">Stops the running-dot breathe and picker transitions.</div>
          </div>
          <button
            type="button"
            className={reduceMotion ? 'pn-btn on' : 'pn-btn'}
            onClick={() => setReduce(!reduceMotion)}
          >
            {reduceMotion ? 'On' : 'Off'}
          </button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">First-run steps</div>
            <div className="pn-detail">Shows them again. The chats stay.</div>
          </div>
          <button type="button" className="pn-btn" onClick={onReplay}>Show</button>
        </div>
      </div>

      {about && <AboutModal onClose={() => setAbout(false)} daemonVersion={capabilities?.daemon ?? null} />}
    </div>
  )
}

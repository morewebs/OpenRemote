// Settings: the web console's own look, stored in this browser. There is no
// daemon to configure here; appearance is the whole sheet.

import { useState } from 'react'
import Toggle from '../Toggle.jsx'
import {
  applyDensity,
  applyPrivacyMode,
  applyReduceMotion,
  loadPrefs,
  savePrefs,
} from '../settings.js'
import '../devices.css'
import '../panels.css'

export default function WebSettingsView() {
  const [prefs, setPrefs] = useState(loadPrefs)
  const set = (partial) => {
    if (!savePrefs(partial)) return false
    setPrefs((current) => ({ ...current, ...partial }))
    return true
  }
  return (
    <div className="panel">
      <header className="dv-head">
        <div>
          <h1 className="dv-title">Settings</h1>
          <p className="dv-meta">The web console's own look, stored in this browser.</p>
        </div>
      </header>
      <h2 className="pn-section">Appearance</h2>
      <p className="pn-lead">Space, not themes - the dark theme is the contract.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">Compact density</div>
            <div className="pn-detail">Pulls in the lists and sidebar rows.</div>
          </div>
          <Toggle
            checked={prefs.density === 'compact'}
            onChange={() => {
              const value = prefs.density === 'compact' ? 'comfortable' : 'compact'
              if (!set({ density: value })) return
              applyDensity(document, value)
            }}
            label="Compact density"
          />
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Reduce motion</div>
            <div className="pn-detail">Stops the running-dot breathe and picker transitions.</div>
          </div>
          <Toggle
            checked={prefs.reduce_motion}
            onChange={() => {
              const value = !prefs.reduce_motion
              if (!set({ reduce_motion: value })) return
              applyReduceMotion(document, value)
            }}
            label="Reduce motion"
          />
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Privacy mode</div>
            <div className="pn-detail">
              Blurs titles, projects, paths, device names, your email and the transcript.
            </div>
          </div>
          <Toggle
            checked={prefs.privacy_mode}
            onChange={() => {
              const value = !prefs.privacy_mode
              if (!set({ privacy_mode: value })) return
              applyPrivacyMode(document, value)
            }}
            label="Privacy mode"
          />
        </div>
      </div>
    </div>
  )
}

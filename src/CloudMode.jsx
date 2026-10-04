// Cloud mode's coming-soon state: the main area while the mode switch
// sits on Cloud. Speaks the app's own status vocabulary (the breathing
// accent dot a waiting machine gets) — not a centered marketing page.

import { Cloud } from '@phosphor-icons/react'
import { House } from '@phosphor-icons/react'
import './cloudmode.css'

export default function CloudMode({ onBackToLocal }) {
  return (
    <div className="cloudmode">
      <header className="cm-head">
        <h1 className="cm-title">Cloud</h1>
        <p className="cm-status">
          <span className="cm-dot" aria-hidden="true" />
          Coming soon
        </p>
      </header>

      <p className="cm-body">
        Run the same harnesses on machines you reach over the network — cloud hosts, home servers,
        anything that checks in. This one stays on <span className="cm-plain">this computer</span> for
        now.
      </p>

      <button type="button" className="cm-back" onClick={onBackToLocal}>
        <House size={13} />
        Back to Local
      </button>
    </div>
  )
}

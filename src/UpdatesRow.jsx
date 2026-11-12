import { useState, useSyncExternalStore } from 'react'
import Toggle from './Toggle.jsx'
import { errorCopy } from './error-copy.js'
import {
  autoCheck,
  checkForUpdates,
  installUpdate,
  setAutoCheck,
  subscribeUpdates,
  updateState,
} from './updates.js'

// Settings > This install: is there a newer OpenRemote, and should the app
// keep looking. Installing always waits for the user's click: the app stops
// its daemon and restarts into the new version, so it never happens under
// them.
export default function UpdatesRow() {
  const s = useSyncExternalStore(subscribeUpdates, updateState)
  const [auto, setAuto] = useState(autoCheck)
  const status = s.installing
    ? "Installing. The app restarts when it's done."
    : s.checking
      ? 'Looking for a newer version…'
      : !s.enabled
        ? "Updates aren't switched on in this build yet. New releases install from the release page."
        : s.available
          ? `Version ${s.available.version} is ready. Your chats stop while the app restarts.`
          : s.error
            ? errorCopy(s.error)
            : s.checkedAt
              ? 'This is the newest version.'
              : 'Looks once a day while the app runs.'
  return (
    <>
      <div className="pn-row" data-stg="install-updates">
        <div>
          <div className="pn-name">Updates</div>
          <div className="pn-detail">{status}</div>
        </div>
        {s.available && s.enabled ? (
          <button type="button" className="pn-btn" onClick={installUpdate} disabled={s.installing}>
            Update and restart
          </button>
        ) : (
          <button type="button" className="pn-btn" onClick={checkForUpdates} disabled={s.checking || s.installing}>
            Check now
          </button>
        )}
      </div>
      <div className="pn-row" data-stg="install-updates-auto">
        <div>
          <div className="pn-name">Look for updates automatically</div>
          <div className="pn-detail">Once a day. Installing still waits for you.</div>
        </div>
        <Toggle
          checked={auto}
          label="Look for updates automatically"
          onChange={() => {
            setAutoCheck(!auto)
            setAuto(!auto)
          }}
        />
      </div>
    </>
  )
}

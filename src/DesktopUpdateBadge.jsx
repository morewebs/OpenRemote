import { useEffect, useState, useSyncExternalStore } from 'react'
import { ArrowCircleUp, ArrowClockwise, CheckCircle, CircleNotch, Info } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { stopTargets } from './stop-everything.js'
import { errorCopy } from './error-copy.js'
import { UpdateConfirm } from './InfoBadge.jsx'
import { aboutUpdateStatus, checkForUpdates, subscribeUpdates, updateState } from './updates.js'

// Shares the desktop updater's feed, staged installer and confirmation.
// Opening About asks for a fresh check without changing the auto-check setting.
export default function DesktopUpdateBadge() {
  const state = useSyncExternalStore(subscribeUpdates, updateState)
  const { sessions, thisDevice } = useConsole()
  const [confirming, setConfirming] = useState(false)
  useEffect(() => { checkForUpdates() }, [])
  const status = aboutUpdateStatus(state)
  const runningChats = stopTargets(sessions ?? [], { thisDevice }).stoppable.length
  const working = status === 'checking' || status === 'installing'

  return (
    <>
      <span className="ab-update-status" role="status" aria-live="polite">
        {working ? (
          <span className="ab-update-badge" aria-busy="true">
            <CircleNotch className="ab-update-spinner" size={16} aria-hidden="true" />
            {status === 'installing' ? 'Installing…' : 'Checking…'}
          </span>
        ) : status === 'current' ? (
          <span className="ab-update-badge"><CheckCircle size={16} weight="fill" aria-hidden="true" /> Up to date</span>
        ) : status === 'available' ? (
          <button
            type="button"
            className="ab-update-badge ab-update-badge--available"
            onClick={() => setConfirming(true)}
            aria-haspopup="dialog"
            aria-label={`Update available: OpenRemote ${state.available.version}`}
            title={`OpenRemote ${state.available.version} is available`}
          >
            <ArrowCircleUp size={16} weight="fill" aria-hidden="true" /> Update available
          </button>
        ) : status === 'error' ? (
          <button type="button" className="ab-update-badge" onClick={checkForUpdates} title={`${errorCopy(state.error)} Click to retry.`}>
            <ArrowClockwise size={16} aria-hidden="true" /> Retry check
          </button>
        ) : (
          <span className="ab-update-badge" title="Signed updates aren't enabled in this build.">
            <Info size={16} aria-hidden="true" /> Updates unavailable
          </span>
        )}
      </span>
      {confirming && state.enabled && state.available && (
        <UpdateConfirm
          available={state.available}
          runningChats={runningChats}
          installing={state.installing}
          onClose={() => setConfirming(false)}
        />
      )}
    </>
  )
}

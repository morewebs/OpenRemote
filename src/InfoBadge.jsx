import { useRef, useState, useSyncExternalStore } from 'react'
import { createPortal } from 'react-dom'
import { Info, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { stopTargets } from './stop-everything.js'
import { errorCopy } from './error-copy.js'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { confirmCopy, installUpdate, subscribeUpdates, updateBadge, updateState } from './updates.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'
import './info-badge.css'

// The top bar's info button. While the shell has a newer release it becomes an
// "Update Available" badge in the same place, and the badge opens the
// confirmation. The badge is the same kind of control as the button, so focus
// and Tab order do not move when it appears.

export default function InfoBadge({ onAbout }) {
  const s = useSyncExternalStore(subscribeUpdates, updateState)
  const { sessions, thisDevice } = useConsole()
  const [confirming, setConfirming] = useState(false)
  // A restart stops the chats this computer runs. A synced chat that runs on
  // another device keeps going, so it is not counted.
  const runningChats = stopTargets(sessions ?? [], { thisDevice }).stoppable.length
  const badge = updateBadge({
    available: s.available,
    enabled: s.enabled,
    installing: s.installing,
    runningChats,
  })
  const version = s.available?.version ?? ''

  // The accessible name starts with the visible label, so a screen reader says
  // what the control shows. It opens the update, not About, so the About name
  // would mislead the person who hears it.
  return (
    <>
      {badge.show ? (
        <button
          type="button"
          className={`ib-badge ib-badge--${badge.tone}`}
          onClick={() => setConfirming(true)}
          title={`OpenRemote ${version} is available`}
          aria-label={`${badge.label}: OpenRemote ${version}`}
          aria-haspopup="dialog"
        >
          {badge.label}
        </button>
      ) : (
        <button
          type="button"
          className="tb-info"
          onClick={onAbout}
          title="About OpenRemote"
          aria-label="About OpenRemote"
        >
          <Info size={14} weight="duotone" />
        </button>
      )}
      {confirming && s.available && (
        <UpdateConfirm
          available={s.available}
          runningChats={runningChats}
          installing={s.installing}
          onClose={() => setConfirming(false)}
        />
      )}
    </>
  )
}

// The confirmation is portalled to the body: the top bar's title is
// pointer-events: none, and that would pass through to the dialog's text and
// backdrop. The dialog plumbing (focus in, Tab kept inside, the app inert
// behind, focus back to the badge) is shared; see dialog-focus.js.
function UpdateConfirm({ available, runningChats, installing, onClose }) {
  useBack(true, onClose)
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  const canInstall = available.can_install === true
  const copy = confirmCopy({ available, runningChats, canInstall })
  const working = busy || installing

  const confirm = async () => {
    setBusy(true)
    setError(null)
    const outcome = await installUpdate()
    if (outcome.ok) {
      // A Windows install restarts the app, so the dialog stays busy until it
      // does. A release page opens and the dialog has nothing more to say.
      if (!canInstall) onClose()
      return
    }
    setError(errorCopy(outcome.error))
    setBusy(false)
  }

  return createPortal(
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={copy.title}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">{copy.title}</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close" aria-label="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        {copy.body.map((line) => (
          <p key={line} className="dv-modal-hint">
            {line}
          </p>
        ))}
        {copy.warning && <p className="ib-warning">{copy.warning}</p>}
        {error && <p className="cv-error-line">{error}</p>}
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn" onClick={onClose} disabled={working}>
            {copy.secondary}
          </button>
          <button
            type="button"
            className={copy.danger ? 'pn-btn pn-btn--danger' : 'pn-btn'}
            onClick={confirm}
            disabled={working}
          >
            {working ? (canInstall ? 'Installing...' : 'Opening...') : copy.primary}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  )
}

import { useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { isMobile } from './platform.js'
import { X } from '@phosphor-icons/react'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'
import { errorCopy } from './error-copy.js'

// Deleting a synced chat is not undoable and reaches every device, so it is
// asked once, in place. Esc / backdrop / X close it like the other modals,
// and Esc here answers only this confirm: a window it was opened over stays
// open behind it. The dialog plumbing (focus in, Tab kept inside, the app
// inert behind, focus back on the opener) is shared; see dialog-focus.js.
export default function DeleteChatModal({ title, onDelete, onClose }) {
  useBack(true, onClose)
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  const confirm = async () => {
    setBusy(true)
    setError(null)
    try {
      await onDelete()
      onClose()
    } catch (err) {
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  const panel = (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Delete chat">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Delete this chat?</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          <strong className="pv">{title}</strong> is deleted here. Any other device it is synced to
          deletes its copy the next time it connects. The files it changed on disk stay where they
          are. This can&apos;t be undone.
        </p>
        {error && <p className="cv-error-line">{error}</p>}
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="pn-btn pn-btn--danger" onClick={confirm} disabled={busy}>
            {busy ? 'Deleting...' : 'Delete everywhere'}
          </button>
        </div>
      </div>
    </div>
  )
  return isMobile ? createPortal(panel, document.body) : panel
}

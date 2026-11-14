import { useRef } from 'react'
import { createPortal } from 'react-dom'
import { X } from '@phosphor-icons/react'
import { isMobile } from './platform.js'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import './view-popup.css'

// One of the app's views (Settings, Metrics, Mobile) in a window of its
// own over the app, so opening it never loses the chat underneath. The
// view keeps its own title; this adds the frame and a way out: the close
// button, Esc, a click outside, or the phone's back gesture.
export default function ViewPopup({ label, onClose, children, bare = false }) {
  useBack(true, onClose)
  const panelRef = useRef(null)
  // Esc closes this window unless a layer on top of it takes the key: a
  // dialog the view opened (a delete confirm, the endpoint manager) answers
  // its own Esc and leaves the window behind it open. A view's own use of
  // Esc, a shortcut being recorded, comes first.
  useDialog(panelRef, onClose)
  // The phone's page moves aside to reveal the drawer. A fixed dialog
  // inside that transformed page would move and clip with it too.
  const popup = (
    <div
      className={`vp-backdrop${isMobile ? ' is-phone' : ''}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="vp-window" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={label}>
        {!bare && (
          <button type="button" className="vp-close" onClick={onClose} title="Close" aria-label="Close">
            <X size={14} weight="bold" />
          </button>
        )}
        <div className="vp-body">{children}</div>
      </div>
    </div>
  )
  return isMobile ? createPortal(popup, document.body) : popup
}

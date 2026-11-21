import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { isMobile } from './platform.js'
import { X } from '@phosphor-icons/react'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { renameState } from './rename-chat.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'
import './rename-chat.css'
import { errorCopy } from './error-copy.js'

// Renaming a chat, in the console, in one field. Every chat starts as "A new
// task" and the only other way to change it is the `/rename` command, which
// nobody discovers; a name is how a chat is found again in a list of thirty.
//
// The shell is the shared modal family: Esc / backdrop / X discard, Tab
// stays inside, and the app behind goes inert (dialog-focus.js). Enter saves.
// The naming rules live in rename-chat.js, tested without a DOM.
//
// The dialog closes only after the daemon accepted the name: a refused rename
// stays open with the daemon's own sentence, because closing would pretend a
// rename had landed and the sidebar would still show the old name.
// `original` is the name the chat was given from its first message, or null
// while it is unknown: an empty field puts it back (the daemon refuses an empty
// name, so "no name" means the chat's own one).
export default function RenameChatModal({ title, original = null, synced = false, onRename, onClose }) {
  useBack(true, onClose)
  const backdropRef = useRef(null)
  const panelRef = useRef(null)
  const noteId = useId()
  useDialog(panelRef, onClose)
  const inputRef = useRef(null)
  const [text, setText] = useState(title ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const verdict = renameState(title, text, original)
  // A synced chat's new name reaches another device when that device next
  // syncs, so the sentence says when it lands instead of promising it is
  // already there - the same rule the remove-a-device hint follows.
  const where = synced
    ? 'Updates on other devices when they sync.'
    : 'Only on this device.'

  // Desktop is ready to type. On a phone, let the person see the whole sheet
  // before they tap the name and bring up the keyboard.
  useEffect(() => {
    const input = inputRef.current
    if (isMobile || !input) return
    input.focus()
    input.select()
  }, [])

  // Some mobile browsers resize only the visual viewport for the keyboard.
  // Keep the sheet in that viewport instead of under the keys.
  useLayoutEffect(() => {
    const viewport = window.visualViewport
    const backdrop = backdropRef.current
    if (!isMobile || !viewport || !backdrop) return
    const fit = () => {
      backdrop.style.setProperty('--rn-viewport-height', viewport.height + 'px')
      backdrop.style.setProperty('--rn-viewport-width', viewport.width + 'px')
      backdrop.style.setProperty('--rn-viewport-top', viewport.offsetTop + 'px')
      backdrop.style.setProperty('--rn-viewport-left', viewport.offsetLeft + 'px')
    }
    fit()
    viewport.addEventListener('resize', fit)
    viewport.addEventListener('scroll', fit)
    return () => {
      viewport.removeEventListener('resize', fit)
      viewport.removeEventListener('scroll', fit)
    }
  }, [])

  const save = async () => {
    if (busy) return
    // Nothing to write: the same name closes the dialog (the name is already
    // the one on screen), and an empty field is refused by the rule below.
    if (verdict.state === 'unchanged') {
      onClose()
      return
    }
    if (verdict.state === 'empty') return
    setBusy(true)
    setError(null)
    try {
      await onRename(verdict.value)
      onClose()
    } catch (err) {
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  const dialog = (
    <div
      className="dv-modal-backdrop"
      data-rename-sheet={isMobile ? '' : undefined}
      ref={backdropRef}
      onPointerDown={isMobile ? (e) => e.stopPropagation() : undefined}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="dv-modal"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="Rename this chat"
      >
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Rename this chat</h2>
          <button type="button" className="dv-modal-close" onClick={onClose} aria-label="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <div className="rn-details">
          <input
            ref={inputRef}
            className="rn-input"
            value={text}
            /* No maxLength: the attribute counts UTF-16 units while the name
               rules count code points, so an emoji name would stop at half the
               allowed length in the field and a paste could be cut mid-surrogate.
               cleanName owns the cap and cuts by character, as the daemon does. */
            aria-label="Chat name"
            aria-describedby={noteId}
            enterKeyHint="done"
            aria-invalid={verdict.state === 'empty' ? 'true' : undefined}
            onChange={(e) => {
              setText(e.target.value)
              // A refused name is refused for what is in the field; typing
              // again is the answer to it, so the old sentence goes.
              setError(null)
            }}
            onKeyDown={(e) => {
              if (e.key !== 'Enter' || e.nativeEvent.isComposing) return
              e.preventDefault()
              save()
            }}
            spellCheck={false}
            disabled={busy}
          />
          <p id={noteId} className={`rn-note${verdict.state === 'empty' ? ' is-bad' : ''}`}>
            {verdict.state === 'empty' ? (
              'A chat needs a name.'
            ) : verdict.state === 'restore' ? (
              <>
                Leave blank to restore the first-message name: <span className="pv">{verdict.value}</span>
              </>
            ) : (
              where
            )}
          </p>
          {error && <p className="cv-error-line" role="alert">{error}</p>}
        </div>
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="button"
            className="pn-btn rn-primary"
            onClick={save}
            disabled={busy || verdict.state === 'empty'}
          >
            {busy ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  )

  // The phone drawer translates the whole app sheet and sits above it. A
  // body portal keeps this editor full width and the drawer inert behind it.
  return isMobile ? createPortal(dialog, document.body) : dialog
}

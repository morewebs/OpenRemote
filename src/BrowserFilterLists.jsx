// "Filter lists", from Settings > Browser: the lists ad and tracker blocking
// can use, each with its licence beside it. Nothing is ticked until the user
// ticks it, and a ticked list is downloaded (from its publisher, over HTTPS)
// only while blocking is on.

import { useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { pickedIds, setAdblock } from './browser-adblock.js'
import { browserCall } from './browser-store.js'
import { errorCopy } from './error-copy.js'
import { useDialog } from './dialog-focus.js'
import { useBack } from './use-back.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'
import './browser-settings.css'

export default function BrowserFilterLists({ status, onClose, onSaved }) {
  const panelRef = useRef(null)
  const [chosen, setChosen] = useState(() => new Set(pickedIds(status)))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  useDialog(panelRef, onClose)
  useBack(true, onClose)

  const toggle = (id, on) => {
    setChosen((now) => {
      const next = new Set(now)
      if (on) next.add(id)
      else next.delete(id)
      return next
    })
  }

  const save = async () => {
    setBusy(true)
    setError(null)
    try {
      const lists = status.lists.filter((l) => chosen.has(l.id)).map((l) => l.id)
      const saved = await setAdblock(browserCall, { lists })
      onSaved?.(saved)
      onClose()
    } catch (err) {
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="dv-modal bw-dialog"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="Filter lists"
      >
        <div className="dv-modal-head">
          <span className="dv-modal-title">Filter lists</span>
          <button type="button" className="dv-modal-close" aria-label="Close" onClick={onClose}>
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="bw-dialog-lead">
          {status.on
            ? 'A ticked list is downloaded from its publisher now, and refreshed weekly while blocking is on.'
            : 'A ticked list is downloaded from its publisher only once blocking is on.'}{' '}
          Lists are kept on this computer only.
        </p>
        {status.lists.map((list) => (
          <label key={list.id} className="bw-clear-row">
            <input
              type="checkbox"
              checked={chosen.has(list.id)}
              onChange={(e) => toggle(list.id, e.target.checked)}
            />
            <span>
              <span className="bw-clear-name">
                {list.title}
                {list.about ? ` - ${list.about}` : ''}
              </span>
              <span className="bw-clear-detail">Licence: {list.licence || 'not stated'}</span>
            </span>
          </label>
        ))}
        {error && (
          <p className="bw-dialog-error" role="alert">
            {error}
          </p>
        )}
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="pn-btn" onClick={save} disabled={busy}>
            {busy ? 'Saving…' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  )
}

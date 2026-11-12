// "Import from another browser", from Settings > Browser: the profiles the
// daemon found on this computer, or a folder the user picks, and which halves
// to bring in. The daemon reads only the bookmarks and history (never sign-ins,
// passwords or site data); bookmarks join Other bookmarks and history stays on
// this computer.

import { useEffect, useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { importLine, loadSources, runImport, sourceLabel } from './browser-import.js'
import { browserCall, refreshBookmarks } from './browser-store.js'
import { canPickFolder, pickFolder } from './pick-folder.js'
import { errorCopy } from './error-copy.js'
import { useDialog } from './dialog-focus.js'
import { useBack } from './use-back.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'
import './browser-settings.css'

export default function BrowserImport({ onClose, onImported }) {
  const panelRef = useRef(null)
  const [sources, setSources] = useState(null)
  const [chosen, setChosen] = useState(null)
  const [folder, setFolder] = useState(null)
  const [bookmarks, setBookmarks] = useState(true)
  const [history, setHistory] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  useDialog(panelRef, onClose)
  useBack(true, onClose)

  useEffect(() => {
    let live = true
    loadSources(browserCall)
      .then((found) => {
        if (!live) return
        setSources(found)
        if (found.length) setChosen(found[0].id)
      })
      .catch((err) => {
        if (!live) return
        setSources([])
        setError(errorCopy(err))
      })
    return () => {
      live = false
    }
  }, [])

  const choose = async () => {
    const path = await pickFolder('Choose a browser profile folder')
    if (path) {
      setFolder(path)
      setChosen(null)
    }
  }

  const start = async () => {
    setBusy(true)
    setError(null)
    try {
      const done = await runImport(browserCall, {
        source: folder ? null : chosen,
        path: folder,
        bookmarks,
        history,
      })
      await refreshBookmarks()
      onImported?.(importLine(done))
      onClose()
    } catch (err) {
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  const ready = (chosen || folder) && (bookmarks || history) && !busy

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
        aria-label="Import from another browser"
      >
        <div className="dv-modal-head">
          <span className="dv-modal-title">Import from another browser</span>
          <button type="button" className="dv-modal-close" aria-label="Close" onClick={onClose}>
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="bw-dialog-lead">
          Only bookmarks and history are read, from a copy; never sign-ins, passwords or site data. Bookmarks join
          Other bookmarks and follow your account, at most 2,000 an import. History stays on this computer.
        </p>
        {sources === null && <p className="bw-dialog-lead">Looking for browsers on this computer…</p>}
        {sources?.length === 0 && !folder && (
          <p className="bw-dialog-lead">No other browser was found in its usual folder on this computer.</p>
        )}
        {sources?.map((source) => (
          <label key={source.id} className="bw-clear-row">
            <input
              type="radio"
              name="browser-import-source"
              checked={!folder && chosen === source.id}
              onChange={() => {
                setFolder(null)
                setChosen(source.id)
              }}
            />
            <span>
              <span className="bw-clear-name">{sourceLabel(source)}</span>
              <span className="bw-clear-detail">
                {[source.bookmarks && 'bookmarks', source.history && 'history'].filter(Boolean).join(' and ')}
              </span>
            </span>
          </label>
        ))}
        {folder && (
          <label className="bw-clear-row">
            <input type="radio" name="browser-import-source" checked readOnly />
            <span>
              <span className="bw-clear-name">A folder you picked</span>
              <span className="bw-clear-detail pv">{folder}</span>
            </span>
          </label>
        )}
        <label className="bw-clear-row">
          <input type="checkbox" checked={bookmarks} onChange={(e) => setBookmarks(e.target.checked)} />
          <span className="bw-clear-name">Bookmarks</span>
        </label>
        <label className="bw-clear-row">
          <input type="checkbox" checked={history} onChange={(e) => setHistory(e.target.checked)} />
          <span className="bw-clear-name">History (the last 90 days)</span>
        </label>
        {error && (
          <p className="bw-dialog-error" role="alert">
            {error}
          </p>
        )}
        <div className="ctx-modal-actions">
          {canPickFolder && (
            <button type="button" className="pn-btn" onClick={choose} disabled={busy}>
              Choose a folder…
            </button>
          )}
          <button type="button" className="pn-btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="pn-btn" onClick={start} disabled={!ready}>
            {busy ? 'Importing…' : 'Import'}
          </button>
        </div>
      </div>
    </div>
  )
}

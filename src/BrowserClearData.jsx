// "Clear browsing data", from Settings > Browser: the reading record over a
// time range, and the browser profile's cookies and site data and its cache.
// The shell clears those two whole, for every site (a time range does not
// reach them), and the dialog says so.

import { useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { CLEAR_RANGES, sinceFor } from './browser-history.js'
import { clearKinds } from './browser-chrome.js'
import { clearHistorySince } from './browser-store.js'
import { errorCopy } from './error-copy.js'
import { useDialog } from './dialog-focus.js'
import { useBack } from './use-back.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'
import './browser-settings.css'

export default function BrowserClearData({ onClose, onCleared }) {
  const panelRef = useRef(null)
  const [range, setRange] = useState('hour')
  const [history, setHistory] = useState(true)
  // Off unless chosen: it signs the user out of most sites.
  const [cookies, setCookies] = useState(false)
  const [cache, setCache] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  useDialog(panelRef, onClose)
  useBack(true, onClose)

  const clear = async () => {
    setBusy(true)
    setError(null)
    try {
      const kinds = clearKinds({ cookies, cache })
      if (kinds.length) {
        const { invoke } = await import('@tauri-apps/api/core')
        await invoke('browser_clear_data', { kinds })
      }
      // The reading record is this console's own: cleared after the shell
      // answered, so a refusal leaves everything as it was.
      if (history) clearHistorySince(sinceFor(range))
      onCleared?.()
      onClose()
    } catch (err) {
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  const rows = [
    {
      on: history,
      set: setHistory,
      name: 'Pages read',
      detail: "This window's reading record, in the time above.",
    },
    {
      on: cookies,
      set: setCookies,
      name: 'Cookies and site data',
      detail: 'Signs you out of most sites. Every site, from any time.',
    },
    {
      on: cache,
      set: setCache,
      name: 'Cached images and files',
      detail: 'Frees space; some pages load more slowly the first time after. From any time.',
    },
  ]

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
        aria-label="Clear browsing data"
      >
        <div className="dv-modal-head">
          <span className="dv-modal-title">Clear browsing data</span>
          <button type="button" className="dv-modal-close" aria-label="Close" onClick={onClose}>
            <X size={14} weight="bold" />
          </button>
        </div>
        <div className="bw-clear-ranges" role="radiogroup" aria-label="Time range">
          {CLEAR_RANGES.map((r) => (
            <button
              key={r.id}
              type="button"
              role="radio"
              aria-checked={range === r.id}
              className={range === r.id ? 'on' : ''}
              onClick={() => setRange(r.id)}
            >
              {r.label}
            </button>
          ))}
        </div>
        {rows.map((row) => (
          <label key={row.name} className="bw-clear-row">
            <input type="checkbox" checked={row.on} onChange={(e) => row.set(e.target.checked)} />
            <span>
              <span className="bw-clear-name">{row.name}</span>
              <span className="bw-clear-detail">{row.detail}</span>
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
          <button
            type="button"
            className="pn-btn pn-btn--danger"
            onClick={clear}
            disabled={busy || !(history || cookies || cache)}
          >
            {busy ? 'Clearing…' : 'Clear data'}
          </button>
        </div>
      </div>
    </div>
  )
}

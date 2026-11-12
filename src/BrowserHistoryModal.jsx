// The pane's history: the pages this console has shown, grouped by day, newest
// first. It is a drawing, not a page: the pane's own menu opens it over the
// pane, a row opens its address in the tab, and clearing it asks once. History
// never leaves this computer.

import { useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { groupByDay, shownVisitUrl, visitLabel } from './browser-history.js'
import { useDialog } from './dialog-focus.js'
import { useBack } from './use-back.js'
import './devices.css'
import './panels.css'

export default function BrowserHistoryModal({ entries, onOpen, onClear, onClose }) {
  const panelRef = useRef(null)
  const [armed, setArmed] = useState(false)
  const groups = groupByDay(entries)
  useDialog(panelRef, onClose)
  useBack(true, onClose)

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="dv-modal bw-history"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="History"
      >
        <div className="dv-modal-head">
          <span className="dv-modal-title">History</span>
          <button type="button" className="dv-modal-close" aria-label="Close history" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        {!entries.length ? (
          <p className="bw-history-empty">Pages you visit show here.</p>
        ) : (
          <div className="bw-history-list">
            {groups.map((group) => (
              <section key={group.label} className="bw-history-day">
                <h3 className="bw-history-label">{group.label}</h3>
                {group.rows.map((entry) => (
                  <button
                    key={`${group.label}-${entry.url}-${entry.at}`}
                    type="button"
                    className="bw-history-row"
                    title={entry.url}
                    onClick={() => onOpen(entry.url)}
                  >
                    <span className="bw-history-title">{visitLabel(entry)}</span>
                    <span className="bw-history-url">{shownVisitUrl(entry.url)}</span>
                  </button>
                ))}
              </section>
            ))}
          </div>
        )}
        <div className="ctx-modal-actions">
          {/* Clearing asks once: the first press arms the word, the second
              clears, and the safe answer (Close) stays the primary. */}
          <button
            type="button"
            className={armed ? 'pn-btn pn-btn--danger' : 'pn-btn'}
            disabled={!entries.length}
            onClick={() => {
              if (!armed) {
                setArmed(true)
                return
              }
              onClear()
              setArmed(false)
            }}
          >
            {armed ? 'Clear it' : 'Clear history'}
          </button>
          <button type="button" className="pn-btn bw-primary" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  )
}

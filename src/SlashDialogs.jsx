// What /help and /status show: the terminal's help screen and status panel,
// as the console's own dialogs. /help lists every command the chat's App
// has here, grouped and searchable, with how each one runs; /status is what
// OpenRemote knows about the chat.

import { useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { X } from '@phosphor-icons/react'
import './devices.css'
import './slash.css'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { useEscLayer } from './esc-layer.js'
import { filterHelp, helpGroups, statusRows } from './slash-actions.js'

export function HelpModal({ app, own, commands, onClose }) {
  useBack(true, onClose)
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  const [query, setQuery] = useState('')
  const groups = useMemo(() => helpGroups({ own, commands }), [own, commands])
  const shown = filterHelp(groups, query)
  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal sl-help" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Commands">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Commands for {app}</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <input
          className="sl-help-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search commands"
          spellCheck={false}
          autoComplete="off"
          aria-label="Search commands"
        />
        <div className="sl-help-list">
          {shown.length === 0 && <p className="dv-modal-hint">No command matches that.</p>}
          {shown.map((g) => (
            <section key={g.title} className="sl-help-group">
              <h3 className="sl-help-title">{g.title}</h3>
              {g.rows.map((r) => (
                <div key={r.name} className={`sl-help-row${r.disabled ? ' is-off' : ''}`}>
                  <span className="sl-help-name">
                    {r.name}
                    {r.hint ? <span className="sl-help-hint"> {r.hint}</span> : null}
                  </span>
                  <span className="sl-help-detail">{r.detail}</span>
                  <span className="sl-help-how">{r.how}</span>
                </div>
              ))}
            </section>
          ))}
        </div>
      </div>
    </div>
  )
}

const PRIVATE_STATUS_ROWS = new Set(['Folder', 'Runs on', 'Allowed for this chat'])

export function StatusModal({ chat, app, permissionLabel, modelLabel, device, onClose }) {
  useBack(true, onClose)
  useEscLayer(true, onClose)
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  const rows = statusRows(chat, { app, permissionLabel, modelLabel, device })
  // The composer lets clicks through its outer layer. The dialog must be
  // outside that layer and the pane so its backdrop and controls take clicks.
  return createPortal(
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal dv-modal--form" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Chat status">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">This chat</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <dl className="sl-status">
          {rows.map(([label, value]) => (
            <div key={label} className="sl-status-row">
              <dt>{label}</dt>
              <dd className={PRIVATE_STATUS_ROWS.has(label) ? 'pv' : undefined}>{value}</dd>
            </div>
          ))}
        </dl>
      </div>
    </div>,
    document.body,
  )
}

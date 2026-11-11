// The cheat sheet (Ctrl+/): every action that has a key, by group, with
// the key as it is bound on this device right now.

import { useRef } from 'react'
import { X } from '@phosphor-icons/react'
import { useKeyboard, isMac } from './keyboard.jsx'
import { useDialog } from './dialog-focus.js'
import { GROUPS, comboLabel, shownActions } from './shortcuts.js'
import { hasTauri, isMobile } from './platform.js'
import './keyboard.css'

// The Browser group shows only where there is a browser: the desktop app.
const BROWSER_KEYS = hasTauri && !isMobile

export default function ShortcutSheet({ onClose }) {
  const kb = useKeyboard()
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)

  const groups = GROUPS.map((g) => ({
    ...g,
    rows: shownActions(BROWSER_KEYS).filter((a) => a.group === g.id && kb.bindings[a.id]),
  })).filter((g) => g.rows.length)

  return (
    <div
      className="kb-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="kb-sheet" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
        <div className="kb-sheet-head">
          <h2>Keyboard shortcuts</h2>
          <button type="button" className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <div className="kb-sheet-grid">
          {groups.map((g) => (
            <section key={g.id} className="kb-sheet-group">
              <div className="kb-group-label">{g.label}</div>
              {g.rows.map((a) => (
                <div key={a.id} className="kb-sheet-row">
                  <span>{a.label}</span>
                  <kbd className="kb-keys">{comboLabel(kb.bindings[a.id], isMac)}</kbd>
                </div>
              ))}
            </section>
          ))}
        </div>
        <p className="kb-sheet-foot">
          Everything else is in the command palette
          {kb.bindings.palette ? ` (${comboLabel(kb.bindings.palette, isMac)})` : ''}. Change any key in
          Settings, under Keyboard.
        </p>
        <p className="kb-sheet-foot">
          Actions that leave the chat (New chat, Archive, Close pane, and the view switches) wait until
          the caret leaves the message box, so a half-written message is never lost.
        </p>
      </div>
    </div>
  )
}

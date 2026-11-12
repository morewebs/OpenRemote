import { useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { chatStatus } from './state/reducer.js'
import { projectName } from './projects.js'
import { busyReason, moveSentence, setAskBeforeMove } from './move-chat.js'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { errorCopy } from './error-copy.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'
import './move.css'

/**
 * Moving a chat to another project, asked first: a move changes where the
 * chat's agent works, so a stray drop or click must not do it alone. One
 * sentence says what changes; "Don't show this again" stops the asking on
 * this device (Settings > Confirmations turns it back on). Esc / backdrop /
 * X / Cancel close it like the other modals; the dialog plumbing is shared
 * (dialog-focus.js), and Move is held while a turn runs.
 *
 * move: { chat: { id, title }, to: project | null, from: project | null,
 *         device: the machine's name when the chat runs on another one }
 * failure: why a move made without asking failed, for a surface with no
 * status line of its own - the dialog then says so and offers to try again.
 */
export default function MoveChatModal({ move, failure = null, onMoved, onClose }) {
  const { moveChat, chats, sessions } = useConsole()
  useBack(true, onClose)
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  const [skip, setSkip] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(failure)
  const { chat, to, from, device } = move
  // Live: a turn that ends while the dialog is open frees the move.
  const status = chats[chat.id]?.status ?? chatStatus((sessions ?? []).find((s) => s.id === chat.id)?.status)
  const blocked = busyReason(status)

  const confirm = async () => {
    setBusy(true)
    setError(null)
    try {
      await moveChat(chat.id, to?.id ?? null)
      // Kept only once the move landed: a refused move leaves the asking on.
      if (skip) setAskBeforeMove(false)
      onMoved?.(move)
      onClose()
    } catch (err) {
      // The daemon's own sentence names the reason (a turn that started, a
      // project that went) better than anything this dialog could.
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  const name = chat.title ?? 'this chat'
  const fromName = from ? projectName(from) : 'its project'

  // In the body: the dialog is opened from the sidebar and from a pane's
  // menu alike, and neither box may clip or shift it.
  return createPortal(
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Move chat">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">
            {failure ? (
              <>
                Couldn&apos;t move <span className="pv">&ldquo;{name}&rdquo;</span>
              </>
            ) : to ? (
              <>
                Move <span className="pv">&ldquo;{name}&rdquo;</span> to <span className="pv">{projectName(to)}</span>?
              </>
            ) : (
              <>
                Move <span className="pv">&ldquo;{name}&rdquo;</span> out of <span className="pv">{fromName}</span>?
              </>
            )}
          </h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint pv">
          {moveSentence({ chatTitle: name, to })}
          {device ? ` It runs on ${device}.` : ''}
        </p>
        {blocked && !error && <p className="cv-error-line">{blocked}</p>}
        {error && <p className="cv-error-line">{error}</p>}
        {!failure && (
          <label className="mv-skip">
            <input type="checkbox" checked={skip} onChange={(e) => setSkip(e.target.checked)} />
            Don&apos;t show this again
          </label>
        )}
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="pn-btn" onClick={confirm} disabled={busy || Boolean(blocked)}>
            {busy ? 'Moving...' : failure ? 'Try again' : 'Move'}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  )
}

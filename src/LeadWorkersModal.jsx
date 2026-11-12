import { useRef } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { descendantsOf, isAlive } from './team.js'
import './devices.css'
import './panels.css'
import './contextmenu.css'

// A chat that has workers is being archived or deleted: what becomes of
// them is the user's call, asked once, in place, wherever the click came
// from (the sidebar, a pane's menu, Settings, the chat's own header). Esc,
// the backdrop and X close it like the other modals - and mean "never mind".
// The dialog plumbing (focus, Tab kept inside, the Esc layer) is shared; see
// dialog-focus.js.
export default function LeadWorkersModal() {
  const { teamAsk } = useConsole()
  return teamAsk ? <Asking ask={teamAsk} /> : null
}

function Asking({ ask }) {
  const { answerTeamAsk, sessions, chats } = useConsole()
  const cancel = () => answerTeamAsk(null)
  useBack(true, cancel)
  const panelRef = useRef(null)
  useDialog(panelRef, cancel)

  const team = descendantsOf(sessions, ask.id)
  const running = team.filter(({ session }) => isAlive(session)).length
  const title = chats[ask.id]?.title ?? sessions.find((s) => s.id === ask.id)?.title ?? 'This chat'
  const workers = `${team.length} worker${team.length === 1 ? '' : 's'}`
  const archive = ask.action === 'archive'

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) cancel()
      }}
    >
      <div
        className="dv-modal"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={archive ? 'Archive with its workers' : 'Delete with its workers'}
      >
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">{archive ? 'Archive with its workers?' : 'Delete with its workers?'}</h2>
          <button className="dv-modal-close" onClick={cancel} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          <strong className="pv">{title}</strong> has {workers}
          {running > 0 ? `, ${running} still running` : ''}.{' '}
          {archive
            ? 'Its workers are other chats this chat started. Archiving hides a chat on this computer; it keeps running.'
            : 'Its workers are other chats this chat started. They keep running unless you delete them too.'}
        </p>
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn" onClick={cancel}>
            Cancel
          </button>
          {archive ? (
            <>
              <button type="button" className="pn-btn" onClick={() => answerTeamAsk('leave')}>
                Archive this chat only
              </button>
              {running > 0 && (
                <button type="button" className="pn-btn" onClick={() => answerTeamAsk('stop')}>
                  Stop and archive all
                </button>
              )}
              <button type="button" className="pn-btn on" onClick={() => answerTeamAsk('with')}>
                Archive all
              </button>
            </>
          ) : (
            <>
              <button type="button" className="pn-btn" onClick={() => answerTeamAsk('keep')}>
                Delete this chat only
              </button>
              <button type="button" className="pn-btn pn-btn--danger" onClick={() => answerTeamAsk('delete')}>
                Delete all
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  )
}

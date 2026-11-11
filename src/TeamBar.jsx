import { useState } from 'react'
import { CaretDown, CaretRight, Stop } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { chatStatus } from './state/reducer.js'
import { HarnessMark } from './brand-marks.jsx'
import { descendantsOf, isAlive, nameOf, stateWord, teamDot, teamSummary } from './team.js'
import { requestBeside } from './team-placement.js'
import './team.css'

/// The team under a chat: its workers, each one's state in the sidebar's
/// dot language, a way into each, and Stop. Shows only for a chat that has
/// workers. A worker is an ordinary chat - clicking its row opens it beside
/// this one, where the user can steer, stop or take it over.
export default function TeamBar({ chat, canAct }) {
  const { sessions, stopChat, stopTeam } = useConsole()
  const [open, setOpen] = useState(true)
  const [error, setError] = useState(null)
  const members = descendantsOf(sessions, chat.id)
  if (members.length === 0) return null
  const running = members.filter(({ session }) => isAlive(session))
  const roll = teamDot(sessions, chat.id)
  const summary = teamSummary(sessions, chat.id)

  const act = async (run) => {
    setError(null)
    try {
      await run()
    } catch (err) {
      setError(err.message ?? String(err))
    }
  }

  return (
    <section className="cv-team" aria-label="Team">
      <div className="cv-team-bar">
        <button
          type="button"
          className="cv-team-head"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
        >
          {open ? <CaretDown size={11} /> : <CaretRight size={11} />}
          <span className="cv-team-title">Team ({members.length})</span>
          {roll && <span className={`dot dot--${roll}`} />}
          {summary && <span className="cv-team-summary">{summary}</span>}
        </button>
        {canAct && running.length > 0 && (
          <button
            type="button"
            className="cv-team-btn"
            onClick={() => act(() => stopTeam(chat.id))}
            title="Stop every worker that is still running - this chat stays up"
          >
            <Stop size={11} weight="fill" />
            Stop all
          </button>
        )}
      </div>
      {open && (
        <div className="cv-team-rows">
          {members.map(({ session, depth }) => (
            <div
              key={session.id}
              className="cv-team-row"
              style={depth > 1 ? { paddingLeft: 6 + (depth - 1) * 16 } : undefined}
            >
              <HarnessMark harness={session.harness} size={13} />
              <button
                type="button"
                className="cv-team-name pv"
                onClick={() => requestBeside({ from: chat.id, chat: session.id })}
                title="Open beside this chat"
              >
                {nameOf(session)}
              </button>
              <span className="cv-team-state">
                <span className={`dot dot--${chatStatus(session.status)}`} />
                {stateWord(session.status)}
              </span>
              {canAct && isAlive(session) && (
                <button
                  type="button"
                  className="cv-team-btn"
                  onClick={() => act(() => stopChat(session.id))}
                  title="Stop this worker"
                >
                  <Stop size={11} weight="fill" />
                  Stop
                </button>
              )}
            </div>
          ))}
        </div>
      )}
      {error && <p className="cv-team-error">{error}</p>}
    </section>
  )
}

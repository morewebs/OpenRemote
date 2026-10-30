// Settings > MCP servers: tools a session can call, set up by hand on a
// machine; a key never leaves it. (Plugins can ship MCP servers of their
// own - those live with their plugin, not here.)

import { useRef, useState } from 'react'
import { createRemovalLock } from './removal-lock.js'
import AddMcpServerModal from './AddMcpServerModal.jsx'
import { useConsole } from './state/console.jsx'
import { armRow, clearRow, rowIsArmed } from './row-confirm.js'
import { useArmedRow } from './use-armed-row.js'
import './devices.css'
import './panels.css'
import { errorCopy } from './error-copy.js'

// The state word the design rules: derived, never stored.
function serverState(p) {
  if (!p.enabled) return 'off'
  if (p.needs_key && !p.has_key) return 'needs-key'
  return 'running'
}

const STATE_LABEL = { running: 'Running', 'needs-key': 'Needs a key', off: 'Off' }

function machineName(machines, id) {
  return (machines ?? []).find((m) => m.machine.id === id)?.machine.name ?? id
}

export default function McpServersSection() {
  const {
    machines,
    mcpServers,
    mcpServersAnswered,
    removeMcpServer,
    setMcpServerEnabled,
    acknowledgeMcpServerKey,
  } = useConsole()
  const [adding, setAdding] = useState(false)
  const [error, setError] = useState(null)
  // The row is the only copy of the launch command and its binding, and a
  // chat that is running would keep starting a server the record no
  // longer describes: the first press arms, the second removes.
  const [armed, setArmed] = useState(null)
  // The row whose removal is in flight, so a second press cannot send a second
  // delete over the same row (the second would answer a 404).
  const [removing, setRemoving] = useState(null)
  // The lock the press mutates before React renders; `removing` is only what
  // the row draws. See src/removal-lock.js.
  const lockRef = useRef(null)
  // The arm ends when the row scrolls out of view or loses focus (see
  // use-armed-row.js), so a first press cannot outlive the user's attention.
  const armWatch = useArmedRow(armed, setArmed)
  const installed = mcpServers ?? []
  // One machine is the honest local state - the plural, the hostname, and
  // the install picker return by themselves when remote machines check in.
  const oneMachine = (machines ?? []).length <= 1
  const machineWord = oneMachine ? 'machine' : 'machines'

  const run = async (action, ...args) => {
    setError(null)
    setArmed(clearRow())
    try {
      await action(...args)
    } catch (err) {
      setError(errorCopy(err))
    }
  }

  const remove = async (id) => {
    // One claim per press: see src/removal-lock.js for why this cannot be the
    // `removing` state.
    if (!lockRef.current) lockRef.current = createRemovalLock()
    if (!lockRef.current.claim(id)) return
    setError(null)
    setArmed(clearRow())
    setRemoving(id)
    try {
      await removeMcpServer(id)
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      lockRef.current.release()
      setRemoving(null)
    }
  }

  return (
    <>
      <p className="pn-lead">
        {oneMachine
          ? 'Tools a session can call, set up by hand. Each runs on this computer, and a key never leaves it.'
          : 'Tools a session can call, set up by hand. Each runs on a machine, and a key never leaves that machine.'}
      </p>
      {installed.length > 0 && (
      <div className="pn-list pn-list--wide">
        {installed.map((p) => {
          const state = serverState(p)
          const host = machineName(machines, p.machine)
          return (
            <div key={p.id} className="pn-row pn-row--stack" data-stg={`mcp-${p.id}`}>
              <div className="pn-row-main">
                <div>
                  {/* The person names these, so the name is theirs, not the
                      catalogue's: it blurs with their other content. */}
                  <div className="pn-name pv">{p.name}</div>
                  <div className="pn-detail">
                    {p.detail}
                    {oneMachine ? '' : <> · <span className="pv">{host}</span></>}
                  </div>
                </div>
                <div className="pn-actions">
                  <span className={`pn-state pn-state--${state}`}>{STATE_LABEL[state]}</span>
                  {state === 'running' && (
                    <button type="button" className="pn-btn" onClick={() => run(() => setMcpServerEnabled(p.id, false))}>
                      Disable
                    </button>
                  )}
                  {state === 'needs-key' && (
                    <button type="button" className="pn-btn on" onClick={() => run(() => acknowledgeMcpServerKey(p.id))}>
                      Add key
                    </button>
                  )}
                  {state === 'off' && (
                    <button type="button" className="pn-btn" onClick={() => run(() => setMcpServerEnabled(p.id, true))}>
                      Enable
                    </button>
                  )}
                  {state === 'needs-key' && (
                    <button type="button" className="pn-btn" onClick={() => run(() => setMcpServerEnabled(p.id, false))}>
                      Disable
                    </button>
                  )}
                  {rowIsArmed(armed, p.id) ? (
                    <button
                      type="button"
                      className="pn-btn pn-btn--danger"
                      ref={armWatch.buttonRef}
                      onBlur={armWatch.onBlur}
                      disabled={removing != null}
                      onClick={() => remove(p.id)}
                      title="Its launch command and binding go with it, and there is no undo"
                    >
                      {removing === p.id ? "Removing…" : "Remove it and its launch command"}
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="pn-btn"
                      disabled={removing != null}
                      onClick={() => setArmed((curr) => armRow(curr, p.id))}
                    >
                      Remove
                    </button>
                  )}
                </div>
              </div>
              <code className="pn-command pv">{p.command}</code>
              {state === 'needs-key' && (
                <p className="pn-note">
                  The key stays on {oneMachine ? 'this computer' : <span className="pv">{host}</span>}. Adding it here only records that {oneMachine ? 'it has' : 'the machine has'} one.
                </p>
              )}
            </div>
          )
        })}
      </div>
      )}
      <div className="pn-list pn-list--tight mcp-add">
        {installed.length === 0 && (
          <p className="pn-empty mcp-none">
            {mcpServersAnswered
              ? `No MCP servers on your ${machineWord} yet.`
              : 'Loading your MCP servers…'}
          </p>
        )}
        <div className="pn-row" data-stg="mcp-add">
          <div>
            <div className="pn-name">Add an MCP server</div>
            <div className="pn-detail">Its launch command, as the machine would start it.</div>
          </div>
          <button type="button" className="pn-btn" onClick={() => setAdding(true)}>
            Add
          </button>
        </div>
      </div>
      {error && <p className="pn-error">{error}</p>}
      {adding && <AddMcpServerModal onClose={() => setAdding(false)} />}
    </>
  )
}

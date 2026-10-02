// One machine's detail: what's really on it. This machine carries its
// real sessions (jump straight into the chat), its real harness
// inventory, its uptime over the last day, and the harnesses that could
// be installed. A waiting machine carries its install command and its
// enrollment token — the check-in consumes both.

import { useEffect, useState } from 'react'
import { Check, Copy, Desktop, Trash, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'

const SLICES = 48
const PLATFORM_LABEL = { windows: 'Windows', macos: 'macOS', linux: 'Linux' }
// The session-status words the machine rows carry — the daemon's own.
const STATUS_LABEL = {
  starting: 'Starting',
  working: 'Running',
  waiting: 'Needs a decision',
  idle: 'Idle',
  stopped: 'Stopped',
  failed: 'Failed',
}
const LIVE = ['starting', 'working']

const pad = (n) => String(n).padStart(2, '0')
const clock = (d) => `${pad(d.getHours())}:${pad(d.getMinutes())}`

function presenceLine(machine) {
  if (machine.status === 'waiting') return 'Waiting for the agent'
  if (machine.status === 'offline') return 'Offline'
  return 'Online'
}

export default function MachineModal({ view, onOpenChat, onClose }) {
  const { installHarness, removeMachine } = useConsole()
  const [busyHarness, setBusyHarness] = useState(null)
  const [removing, setRemoving] = useState(false)
  const [error, setError] = useState(null)
  const [copied, setCopied] = useState(false)
  const { machine, harnesses, sessions, presence, installable, install_command: installCommand } = view

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const waiting = machine.status === 'waiting'
  const installed = (harnesses ?? []).filter((h) => h.available)
  const upSlices = (presence ?? []).filter(Boolean).length
  const pct = ((upSlices / SLICES) * 100).toFixed(1)
  const now = Date.now()

  const install = async (harnessId) => {
    if (busyHarness) return
    setBusyHarness(harnessId)
    setError(null)
    try {
      await installHarness(machine.id, harnessId)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusyHarness(null)
    }
  }

  const remove = async () => {
    if (removing) return
    setRemoving(true)
    setError(null)
    try {
      await removeMachine(machine.id)
      onClose()
    } catch (err) {
      setError(err.message ?? String(err))
      setRemoving(false)
    }
  }

  const copyCommand = async () => {
    try {
      await navigator.clipboard.writeText(installCommand ?? '')
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      /* clipboard unavailable */
    }
  }

  const openChat = (chatId) => {
    onClose()
    onOpenChat?.(chatId)
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal dv-devmodal" role="dialog" aria-modal="true" aria-label={machine.name}>
        <div className="dv-modal-head">
          <div className="dv-dev-id">
            <span className="dv-icon">
              <Desktop size={19} weight="light" />
            </span>
            <div>
              <h2 className="dv-modal-title">{machine.name}</h2>
              <div className="dv-presence">
                <span className={`dv-pdot dv-pdot--${machine.status}`} />
                {presenceLine(machine)}
              </div>
              <div className="dv-spec">{PLATFORM_LABEL[machine.platform] ?? machine.platform}</div>
            </div>
          </div>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>

        {waiting ? (
          <>
            <p className="dv-waitnote">
              Run the install command on this machine. It comes online when the agent checks in.
              Nothing can be installed until then.
            </p>
            <div className="dv-modal-cmd">
              <code>{installCommand}</code>
              <button className="dv-modal-copy" onClick={copyCommand} title="Copy command">
                {copied ? <Check size={13} weight="bold" /> : <Copy size={13} />}
              </button>
            </div>
            <p className="dv-token">
              Enrollment token <code>{machine.enrollment_token}</code>
            </p>
          </>
        ) : (
          <>
            <div className="dv-label-row">
              <span>Uptime · last 24h</span>
              <span className="dv-label-val dv-label-val--up">{pct}%</span>
            </div>
            <div className="dv-band">
              {(presence ?? []).map((up, i) => {
                const start = new Date(now - (SLICES - i) * 30 * 60000)
                const end = new Date(now - (SLICES - 1 - i) * 30 * 60000)
                return (
                  <span
                    key={i}
                    className={up ? 'dv-slice' : 'dv-slice down'}
                    title={`${clock(start)} – ${clock(end)} · ${up ? 'Up' : 'Down'}`}
                  />
                )
              })}
            </div>

            <div className="dv-label-row">
              <span>Sessions</span>
              <span className="dv-label-val">{sessions?.length ?? 0}</span>
            </div>
            <div className="dv-agents">
              {(sessions ?? []).length === 0 && <p className="dv-empty">No sessions on this machine.</p>}
              {(sessions ?? []).map((s) => (
                <button key={s.id} className="dv-agent" onClick={() => openChat(s.id)}>
                  <span className={`dv-agent-dot${LIVE.includes(s.status) ? '' : ' idle'}`} />
                  <div className="dv-agent-body">
                    <div className="dv-agent-top">
                      <span className="dv-agent-name">{harnessName(s.harness)}</span>
                      <span className="dv-agent-status">{STATUS_LABEL[s.status] ?? s.status}</span>
                    </div>
                    <div className="dv-agent-sub">
                      {s.model ?? `${harnessName(s.harness)} default`}
                    </div>
                  </div>
                </button>
              ))}
            </div>

            <div className="dv-label-row">
              <span>Harnesses on this machine</span>
              <span className="dv-label-val">{installed.length}</span>
            </div>
            <div className="dv-harnesses">
              {installed.length === 0 && <p className="dv-empty">Nothing installed yet.</p>}
              {installed.map((h) => (
                <span key={h.id} className="dv-chip">
                  {h.name}
                </span>
              ))}
            </div>

            {(installable ?? []).length > 0 && (
              <div className="dv-installs">
                {installable.map((spec) => (
                  <div key={spec.harness_id} className="dv-install">
                    <div className="dv-install-body">
                      <span>{spec.name}</span>
                      <code className="dv-install-cmd">{spec.command}</code>
                    </div>
                    <button
                      type="button"
                      className="dv-act"
                      disabled={busyHarness != null}
                      onClick={() => install(spec.harness_id)}
                    >
                      {busyHarness === spec.harness_id ? 'Installing…' : 'Install'}
                    </button>
                  </div>
                ))}
              </div>
            )}
          </>
        )}

        {error && <p className="dv-error">{error}</p>}

        {!machine.this_machine && (
          <div className="dv-actions">
            <button
              className="dv-act"
              onClick={remove}
              disabled={removing}
              title="Take this machine off the list"
            >
              <Trash size={14} />
              {removing ? 'Removing…' : 'Remove'}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}

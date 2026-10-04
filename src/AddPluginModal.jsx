// Add a plugin by hand: a tool that is not in the marketplace. The
// command is what that machine would start; a key, if it needs one,
// stays there.

import { useEffect, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import './devices.css'

const STATUS = { online: 'Online', offline: 'Offline', waiting: 'Has not checked in' }

export default function AddPluginModal({ onClose }) {
  const { machines, plugins, installPlugin } = useConsole()
  const online = (machines ?? []).find((m) => m.machine.status === 'online')
  const [name, setName] = useState('')
  const [detail, setDetail] = useState('')
  const [command, setCommand] = useState('')
  const [machineId, setMachineId] = useState(online?.machine.id ?? null)
  const [needsKey, setNeedsKey] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const machine = (machines ?? []).find((m) => m.machine.id === machineId)?.machine ?? null
  const ready = name.trim() && detail.trim() && command.trim() && machine?.status === 'online'
  const duplicate =
    ready &&
    (plugins ?? []).some(
      (p) => p.machine === machineId && p.name.toLowerCase() === name.trim().toLowerCase(),
    )

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const oneMachine = (machines ?? []).filter((m) => m.machine.status === 'online').length <= 1
  const hint = duplicate
    ? `${name.trim()} is already on ${oneMachine ? 'this computer' : machine.name}.`
    : oneMachine
      ? 'The command is what this computer starts. A key, if it needs one, stays here.'
      : 'The command is what that machine would start. A key, if it needs one, stays there.'

  const add = async () => {
    if (!ready || duplicate || busy) return
    setBusy(true)
    setError(null)
    try {
      await installPlugin(machineId, {
        name: name.trim(),
        detail: detail.trim(),
        command: command.trim(),
        needsKey,
      })
      onClose()
    } catch (err) {
      setError(err.message ?? String(err))
      setBusy(false)
    }
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal dv-modal--form" role="dialog" aria-modal="true" aria-label="Add plugin">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Add plugin</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">For a tool that is not in the marketplace.</p>
        <label className="dv-host">
          Name
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Notes"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <label className="dv-host">
          What it does
          <input
            value={detail}
            onChange={(e) => setDetail(e.target.value)}
            placeholder="Read the notes repo"
            spellCheck={false}
          />
        </label>
        <label className="dv-host">
          Command
          <input
            className="dv-command"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="npx -y @scope/server"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        {!oneMachine && (
          <div className="dv-host">
            Machine
            <div className="pn-pick">
              {(machines ?? []).map((m) => (
                <button
                  key={m.machine.id}
                  type="button"
                  disabled={m.machine.status !== 'online'}
                  className={m.machine.id === machineId ? 'on' : ''}
                  onClick={() => setMachineId(m.machine.id)}
                >
                  <span className="pn-pick-name">{m.machine.name}</span>
                  <span>{STATUS[m.machine.status] ?? m.machine.status}</span>
                </button>
              ))}
            </div>
          </div>
        )}
        <button
          type="button"
          className={`dv-check${needsKey ? ' on' : ''}`}
          aria-pressed={needsKey}
          onClick={() => setNeedsKey((v) => !v)}
        >
          {oneMachine ? 'It needs a key' : 'It needs a key on that machine'}
        </button>
        <p className="dv-host-hint">{hint}</p>
        {error && <p className="pn-error">{error}</p>}
        <button
          type="button"
          className="dv-act dv-connect"
          disabled={!ready || duplicate || busy}
          onClick={add}
        >
          {busy ? 'Adding…' : 'Add plugin'}
        </button>
      </div>
    </div>
  )
}

import { useEffect, useState } from 'react'
import { X } from '@phosphor-icons/react'
import './devices.css'

const STATUS = { online: 'Online', offline: 'Offline', waiting: 'Has not checked in' }

export default function AddPluginModal({ devices, plugins, onClose, onAdd }) {
  const online = devices.find((d) => d.status === 'online')
  const [name, setName] = useState('')
  const [detail, setDetail] = useState('')
  const [command, setCommand] = useState('')
  const [deviceId, setDeviceId] = useState(online?.id ?? null)
  const [needsKey, setNeedsKey] = useState(false)
  const device = devices.find((d) => d.id === deviceId)
  const ready = name.trim() && detail.trim() && command.trim() && device?.status === 'online'
  const duplicate = ready && plugins.some((p) => p.deviceId === deviceId && p.name.toLowerCase() === name.trim().toLowerCase())

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const hint = duplicate
    ? `${name.trim()} is already on ${device.name}.`
    : 'The command is what that machine would start. A key, if it needs one, stays there.'

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
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Notes" spellCheck={false} autoComplete="off" />
        </label>
        <label className="dv-host">
          What it does
          <input value={detail} onChange={(e) => setDetail(e.target.value)} placeholder="Read the notes repo" spellCheck={false} />
        </label>
        <label className="dv-host">
          Command
          <input className="dv-command" value={command} onChange={(e) => setCommand(e.target.value)} placeholder="npx -y @scope/server" spellCheck={false} autoComplete="off" />
        </label>
        <div className="dv-host">
          Machine
          <div className="pn-pick">
            {devices.map((d) => (
              <button
                key={d.id}
                type="button"
                disabled={d.status !== 'online'}
                className={d.id === deviceId ? 'on' : ''}
                onClick={() => setDeviceId(d.id)}
              >
                <span className="pn-pick-name">{d.name}</span>
                <span>{STATUS[d.status] ?? d.status}</span>
              </button>
            ))}
          </div>
        </div>
        <button type="button" className={`dv-check${needsKey ? ' on' : ''}`} aria-pressed={needsKey} onClick={() => setNeedsKey((v) => !v)}>
          It needs a key on that machine
        </button>
        <p className="dv-host-hint">{hint}</p>
        <button
          type="button"
          className="dv-act dv-connect"
          disabled={!ready || duplicate}
          onClick={() => onAdd({ name, detail, command, deviceId, needsKey })}
        >
          Add plugin
        </button>
      </div>
    </div>
  )
}

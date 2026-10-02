// Add a machine: pick the OS (its install command), name it (the grid's
// hostname slug). The machine lands waiting — it comes online when its
// agent checks in, never from here.

import { useEffect, useState } from 'react'
import { AppleLogo, Check, Copy, LinuxLogo, WindowsLogo, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'

const OS_OPTIONS = [
  { id: 'macos', name: 'macOS', icon: AppleLogo, cmd: 'curl -fsSL openremote.space/install | sh' },
  { id: 'linux', name: 'Linux', icon: LinuxLogo, cmd: 'curl -fsSL openremote.space/install | sh' },
  { id: 'windows', name: 'Windows', icon: WindowsLogo, cmd: 'irm openremote.space/install.ps1 | iex' },
]

function slug(name) {
  return (
    String(name ?? '')
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '')
  )
}

export default function AddMachineModal({ onClose }) {
  const { machines, createMachine } = useConsole()
  const [os, setOs] = useState('linux')
  const [name, setName] = useState('')
  const [copied, setCopied] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const current = OS_OPTIONS.find((o) => o.id === os)
  const hostname = slug(name)
  const duplicate = hostname && (machines ?? []).some((m) => m.machine.name === hostname)

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(current.cmd)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      /* clipboard unavailable */
    }
  }

  const add = async () => {
    if (!hostname || duplicate || busy) return
    setBusy(true)
    setError(null)
    try {
      await createMachine(hostname, os)
      onClose()
    } catch (err) {
      setError(err.message ?? String(err))
      setBusy(false)
    }
  }

  const hint = !hostname
    ? 'The machine stays waiting until its agent checks in.'
    : duplicate
      ? `${hostname} is already in the list.`
      : name.trim() !== hostname
        ? `It will show up as ${hostname}.`
        : 'The machine stays waiting until its agent checks in.'

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal" role="dialog" aria-modal="true" aria-label="Add a machine">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Add a machine</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          Run this on the machine, then name it. It does not come online from here.
        </p>
        <div className="dv-os" role="group" aria-label="Operating system">
          {OS_OPTIONS.map((o) => {
            const Icon = o.icon
            return (
              <button
                key={o.id}
                className={o.id === os ? 'on' : ''}
                aria-pressed={o.id === os}
                onClick={() => setOs(o.id)}
              >
                <Icon size={13} />
                {o.name}
              </button>
            )
          })}
        </div>
        <div className="dv-modal-cmd">
          <code>{current.cmd}</code>
          <button className="dv-modal-copy" onClick={copy} title="Copy command">
            {copied ? <Check size={13} weight="bold" /> : <Copy size={13} />}
          </button>
        </div>
        <label className="dv-host">
          Hostname
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="build-box"
            spellCheck={false}
            autoComplete="off"
            aria-label="Hostname"
          />
          <span className="dv-host-hint">{hint}</span>
        </label>
        {error && <p className="dv-error">{error}</p>}
        <button
          type="button"
          className="dv-act dv-connect"
          disabled={!hostname || duplicate || busy}
          onClick={add}
        >
          {busy ? 'Adding…' : 'Add machine'}
        </button>
      </div>
    </div>
  )
}

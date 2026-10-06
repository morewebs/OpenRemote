// The harness manager: the inventory of all six harnesses on this
// machine, installed or not. The installed rows carry the daemon's own
// probe words (version, path, sign-in status); the missing ones either
// install through npm right here or say the honest truth about their
// own installer. Opened from the harness picker's footer row.

import { useEffect, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { HarnessMark } from './brand-marks.jsx'
import { harnessRows } from './harness-manager.js'
import './devices.css'

export default function HarnessManagerModal({ onClose }) {
  const { capabilities, machines, installHarness } = useConsole()
  const [busy, setBusy] = useState(null)
  const [error, setError] = useState(null)

  // The install specs live on this machine's own view - the daemon's
  // probe of where npm can land a CLI. Null while the machines list
  // hasn't answered; the rows stay quiet about missing harnesses then.
  const thisMachine = machines?.find((view) => view.machine?.this_machine) ?? null
  const rows = harnessRows(capabilities, thisMachine ? thisMachine.installable : null)
  const machineId = thisMachine?.machine?.id ?? null

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const install = async (harnessId) => {
    if (busy || !machineId) return
    setBusy(harnessId)
    setError(null)
    try {
      await installHarness(machineId, harnessId)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(null)
    }
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal dv-devmodal" role="dialog" aria-modal="true" aria-label="Harnesses">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Harnesses</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          What the daemon can drive on this machine. Install a harness&apos;s own CLI and it
          appears here.
        </p>
        <div className="dv-hm-list">
          {rows.map((row) => (
            <div key={row.id} className="dv-hm-row">
              <span className="dv-hm-mark">
                <HarnessMark harness={row.id} size={18} />
              </span>
              <div className="dv-hm-body">
                <div className="dv-hm-top">
                  <span className="dv-hm-name">{row.name}</span>
                  <span className={`dv-hm-status${row.ready ? ' ready' : ''}`}>
                    {row.statusLabel}
                  </span>
                </div>
                {row.detail &&
                  // Mono is for technical content (DESIGN.md): the
                  // daemon's probe words and npm commands are mono, the
                  // prose notes are plain text.
                  (row.detailKind === 'mono' ? (
                    <code className="dv-hm-detail">{row.detail}</code>
                  ) : (
                    <span className="dv-hm-detail dv-hm-note">{row.detail}</span>
                  ))}
              </div>
              {row.action === 'install' && (
                <button
                  type="button"
                  className="dv-act"
                  disabled={busy != null}
                  onClick={() => install(row.id)}
                >
                  {busy === row.id ? 'Installing…' : 'Install'}
                </button>
              )}
            </div>
          ))}
        </div>
        {error && <p className="dv-error">{error}</p>}
      </div>
    </div>
  )
}

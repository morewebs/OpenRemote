// The harness manager: the inventory of all six harnesses on this
// machine, installed or not. Installed rows carry the daemon's own probe
// words (version, path, sign-in status); missing ones install right here
// through their owner's own installer. Opened from the harness picker's
// footer row.

import { useEffect, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { HarnessMark } from './brand-marks.jsx'
import { harnessRows } from './harness-manager.js'
import './devices.css'
import { useBack } from './use-back.js'

export default function HarnessManagerModal({ onClose, onSignIn }) {
  useBack(true, onClose)
  const { capabilities, machines, installHarness } = useConsole()
  const [busy, setBusy] = useState(null)
  const [error, setError] = useState(null)
  // The harness waiting on the runtime go-ahead (pi and its Node.js).
  const [confirming, setConfirming] = useState(null)

  // The install specs live on this machine's own view - the daemon's
  // probe of what each owner's installer can land here. Null while the
  // machines list hasn't answered; missing rows stay quiet then.
  const thisMachine = machines?.find((view) => view.machine?.this_machine) ?? null
  const rows = harnessRows(capabilities, thisMachine ? thisMachine.installable : null)
  const machineId = thisMachine?.machine?.id ?? null

  // Esc closes the topmost layer only: the go-ahead first, then the
  // manager.
  useEffect(() => {
    const onKey = (e) => {
      if (e.key !== 'Escape') return
      if (confirming) setConfirming(null)
      else onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose, confirming])

  const install = async (harnessId, options) => {
    if (busy || !machineId) return
    setConfirming(null)
    setBusy(harnessId)
    setError(null)
    try {
      await installHarness(machineId, harnessId, options)
    } catch (err) {
      setError({ harnessId, message: err.message ?? String(err) })
    } finally {
      setBusy(null)
    }
  }

  const confirmRow = confirming && rows.find((row) => row.id === confirming)

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
          The coding agents on this machine. Install any that are missing - each one
          comes from its maker&apos;s official installer.
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
                    {busy === row.id ? 'Installing…' : row.statusLabel}
                  </span>
                </div>
                {row.detail &&
                  // Mono is for technical content (DESIGN.md): the
                  // daemon's probe words are mono, prose is plain text.
                  (row.detailKind === 'mono' ? (
                    <code className="dv-hm-detail" title={row.detail}>
                      {row.detail}
                    </code>
                  ) : (
                    <span className="dv-hm-detail dv-hm-note">{row.detail}</span>
                  ))}
                {error?.harnessId === row.id && (
                  <div className="dv-error dv-hm-error">
                    <pre>{error.message}</pre>
                  </div>
                )}
              </div>
              {(row.action === 'install' || row.action === 'install-runtime') && (
                <button
                  type="button"
                  className="dv-act"
                  disabled={busy != null}
                  onClick={() =>
                    row.action === 'install-runtime' ? setConfirming(row.id) : install(row.id)
                  }
                >
                  {busy === row.id ? 'Installing…' : 'Install'}
                </button>
              )}
              {row.action === 'signin' && onSignIn && (
                <button type="button" className="dv-act" onClick={() => onSignIn(row.id)}>
                  Sign in
                </button>
              )}
            </div>
          ))}
        </div>
      </div>

      {confirmRow && (
        <RuntimeConfirm
          name={confirmRow.name}
          onCancel={() => setConfirming(null)}
          onConfirm={() => install(confirmRow.id, { withRuntime: true })}
        />
      )}
    </div>
  )
}

// The go-ahead for pi's runtime. Matter-of-fact on purpose: setting up
// Node.js is a normal part of installing pi (its own installer offers
// the same step in a terminal), not a problem to warn about.
function RuntimeConfirm({ name, onCancel, onConfirm }) {
  return (
    <div
      className="dv-modal-backdrop dv-hm-stack"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel()
      }}
    >
      <div className="dv-modal dv-hm-confirm" role="dialog" aria-modal="true" aria-label={`Install ${name}`}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">One more piece for {name}</h2>
          <button className="dv-modal-close" onClick={onCancel} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          {name} runs on Node.js. OpenRemote will set up the official Node.js 22 for it, in{' '}
          {name}&apos;s own folder - the same place {name}&apos;s installer puts it.
        </p>
        <div className="dv-actions">
          <button type="button" className="dv-act primary" autoFocus onClick={onConfirm}>
            Install {name}
          </button>
          <button type="button" className="dv-act" onClick={onCancel}>
            Not now
          </button>
        </div>
      </div>
    </div>
  )
}

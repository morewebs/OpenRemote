// The harness manager: the inventory of all six harnesses on this
// machine, installed or not. Installed rows carry the daemon's own probe
// words (version, path, sign-in status); missing ones install right here
// through their owner's own installer. Opened from the harness picker's
// footer row.

import { useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { HarnessMark } from './brand-marks.jsx'
import { harnessRows } from './harness-manager.js'
import { harnessName } from './harness-names.js'
import { usePrivacyMode } from './privacy.js'
import './devices.css'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { errorCopy } from './error-copy.js'

export default function HarnessManagerModal({ onClose, onSignIn }) {
  const privacy = usePrivacyMode()
  useBack(true, onClose)
  const panelRef = useRef(null)
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

  // Esc closes the topmost thing here: the go-ahead first, then the
  // manager. The go-ahead is a dialog of its own, so it holds its own layer
  // and this one is simply not top while it is open - the layer stack does the
  // ordering that used to be a condition in this handler.
  //
  // An install in flight keeps the modal open: its failure is quoted inside, so closing
  // it mid-install threw the daemon's words away and left only "Not installed" behind.
  const canClose = busy == null
  const closeOrWait = () => {
    if (canClose) onClose()
  }
  useDialog(panelRef, closeOrWait)

  const install = async (harnessId, options) => {
    if (busy || !machineId) return
    setConfirming(null)
    setBusy(harnessId)
    setError(null)
    try {
      await installHarness(machineId, harnessId, options)
    } catch (err) {
      setError({ harnessId, message: errorCopy(err) })
    } finally {
      setBusy(null)
    }
  }

  const confirmRow = confirming && rows.find((row) => row.id === confirming)

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) closeOrWait()
      }}
    >
      <div className="dv-modal dv-devmodal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Harnesses">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Harnesses</h2>
          <button
            className="dv-modal-close"
            onClick={closeOrWait}
            title={canClose ? 'Close' : 'An install is running, so this stays open until it answers'}
          >
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          The coding agents on this machine. Install any that are missing - each one
          comes from its maker&apos;s official installer.
        </p>
        {busy != null && (
          <p className="dv-modal-hint" role="status">
            One install at a time: {harnessName(busy)} is installing. This stays open until the daemon
            answers, so its words are not lost.
          </p>
        )}
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
                  // Mono is for technical content: the
                  // daemon's probe words are mono, prose is plain text.
                  (row.detailKind === 'mono' ? (
                    <code className="dv-hm-detail pv" title={privacy ? undefined : row.detail}>
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
                  // A second install's button went dead beside a row still reading "Not
                  // installed", with the reason nowhere on screen.
                  title={
                    busy != null && busy !== row.id
                      ? 'One install at a time: another harness is installing'
                      : undefined
                  }
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
              {/* The same row without a handler, which is how onboarding opens this modal:
                  a signed-out claude, codex or grok showed the state and nothing at all to
                  do about it. The sentence names where the button is. */}
              {row.action === 'signin' && !onSignIn && (
                <span className="dv-hm-detail dv-hm-note">Sign in from New chat.</span>
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
// the same step in a terminal), not a problem to warn about. It is a dialog
// in its own right - focus in, the app and the panel under it inert, its own
// Esc layer - so backing out of it never takes the manager with it.
function RuntimeConfirm({ name, onCancel, onConfirm }) {
  const panelRef = useRef(null)
  useDialog(panelRef, onCancel)
  return (
    <div
      className="dv-modal-backdrop dv-hm-stack"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel()
      }}
    >
      <div
        className="dv-modal dv-hm-confirm"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={`Install ${name}`}
      >
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

// One device of the user's Cloud: what it is, the synced chats it runs
// (from this computer's copies, so they show even while it's offline),
// and, for a machine that's online, its harnesses - installed and signed
// in remotely, each by its own installer and its own login. Removing a
// device takes it out of the Cloud: the chats it ran stay on it until they
// are deleted there, and its Local chats stay. Leaving Cloud from this
// computer is the one that deletes anything now - the synced copies held
// here - and the confirm says how many.

import { useEffect, useRef, useState } from 'react'
import { Trash, X } from '@phosphor-icons/react'
import SignInModal from './SignInModal.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { SIGNIN_HARNESSES } from './harness-manager.js'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { isPhone, kindLabel, platformLabel, selfLabel } from './cloud.js'
import { errorCopy } from './error-copy.js'

const STATUS_LABEL = {
  starting: 'Starting',
  working: 'Running',
  waiting: 'Needs a decision',
  idle: 'Idle',
  stopped: 'Stopped',
  failed: 'Failed',
}
const LIVE = ['starting', 'working']

function seen(device) {
  if (device.online) return 'Online now'
  if (!device.last_seen_at) return 'Offline'
  const when = new Date(device.last_seen_at * 1000)
  return `Offline · last seen ${when.toLocaleString()}`
}

export default function MachineModal({ device, onOpenChat, onClose }) {
  useBack(true, onClose)
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  const { sessions, thisDevice, apiFor, setDeviceKind, removeDevice, machines } = useConsole()
  const [view, setView] = useState(null)
  const [busyHarness, setBusyHarness] = useState(null)
  const [signInFor, setSignInFor] = useState(null)
  const [confirmRemove, setConfirmRemove] = useState(false)
  // Bumped to ask the machine again after a failed read.
  const [retry, setRetry] = useState(0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const isMe = device.id === thisDevice
  // A phone has no harnesses to show or install.
  const reachable = !isPhone(device) && (isMe || (device.online && device.kind === 'machine'))
  const sessionRows = (sessions ?? []).filter((s) => s.executor === device.id)
  // Every synced session this console holds a copy of: what leaving Cloud
  // deletes here, and what the counts below are about. A copy of a chat that
  // runs elsewhere is deleted along with the ones that run here.
  const syncedCopies = (sessions ?? []).filter((s) => s.executor).length
  const chatCount = (n) => `${n} synced ${n === 1 ? 'chat' : 'chats'}`

  // The device's own machine view: this computer's from the poll, a
  // machine's over the mesh.
  const load = async () => {
    if (isMe) {
      setView((machines ?? []).find((m) => m.machine.this_machine) ?? null)
      return
    }
    if (!reachable) return
    try {
      const list = await apiFor(device.id).machines()
      setView((list ?? [])[0] ?? null)
      setError(null)
    } catch (err) {
      setError(errorCopy(err))
      throw err
    }
  }
  // A machine's first answer can fail while its relay link comes up; ask
  // again every few seconds rather than staying empty until reopened.
  useEffect(() => {
    let timer = null
    let cancelled = false
    load().catch(() => {
      if (!cancelled) timer = setTimeout(() => setRetry((n) => n + 1), 3000)
    })
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [device.id, reachable, machines, retry])

  const act = async (action) => {
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setBusy(false)
    }
  }

  const install = async (harnessId) => {
    if (busyHarness || !view) return
    setBusyHarness(harnessId)
    setError(null)
    try {
      await apiFor(isMe ? null : device.id).installHarness(view.machine.id, harnessId)
      await load().catch(() => {})
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setBusyHarness(null)
    }
  }

  const installed = (view?.harnesses ?? []).filter((h) => h.available)
  const canChangeKind = device.created_via !== 'enrollment'

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal dv-devmodal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={device.name}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title pv">{device.name}</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          {[platformLabel(device.platform), kindLabel(device), device.app_version]
            .filter(Boolean)
            .join(' · ')}
          {' · '}
          {isMe ? selfLabel(device) : seen(device)}
        </p>
        {device.problem === 'key_changed' && (
          <p className="dv-error">
            This device's key changed since it joined, so nothing connects to it. Remove it and add it again.
          </p>
        )}

        <div className="dv-label-row">
          <span>Chats it runs</span>
          <span className="dv-label-val">{sessionRows.length}</span>
        </div>
        <div className="dv-agents">
          {sessionRows.length === 0 && <p className="dv-empty">No synced chats run here.</p>}
          {sessionRows.map((s) => (
            <button
              key={s.id}
              className="dv-agent"
              onClick={() => {
                onClose()
                onOpenChat(s.id)
              }}
            >
              <span className={`dv-agent-dot${LIVE.includes(s.status) ? '' : ' idle'}`} />
              <div className="dv-agent-body">
                <div className="dv-agent-top">
                  <span className={s.title ? 'dv-agent-name pv' : 'dv-agent-name'}>{s.title ?? harnessName(s.harness)}</span>
                  <span className="dv-agent-status">{STATUS_LABEL[s.status] ?? s.status}</span>
                </div>
                <div className="dv-agent-sub">{harnessName(s.harness)}</div>
              </div>
            </button>
          ))}
        </div>

        {reachable && view && (
          <>
            <div className="dv-label-row">
              <span>Apps</span>
              <span className="dv-label-val">{installed.length}</span>
            </div>
            <div className="dv-installs">
              {installed.length === 0 && <p className="dv-empty">Nothing installed yet.</p>}
              {installed.map((h) => (
                <div key={h.id} className="dv-install">
                  <div className="dv-install-body">
                    <span>{harnessName(h.id)}</span>
                    <span className="dv-agent-sub">{h.using_api_key && h.signed_in !== true ? 'Using API key' : h.signed_in === false ? 'Not signed in' : 'Ready'}</span>
                  </div>
                  {h.signed_in === false && SIGNIN_HARNESSES.has(h.id) && (
                    <button type="button" className="dv-act" onClick={() => setSignInFor(h.id)}>
                      Sign in
                    </button>
                  )}
                </div>
              ))}
              {(view.installable ?? []).map((spec) => (
                <div key={spec.harness_id} className="dv-install">
                  <div className="dv-install-body">
                    <span>{spec.name}</span>
                    {/* The installer line is the daemon's own: Install runs it
                        for you, so it stays folded away instead of reading as
                        homework. A disclosure rather than a tooltip keeps it
                        reachable by keyboard and copyable by hand. */}
                    <details className="dv-install-more">
                      <summary>Show the command</summary>
                      <p className="dv-install-why">
                        The installer's own line, for running it yourself.
                      </p>
                      <code className="dv-install-cmd">{spec.command}</code>
                    </details>
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
          </>
        )}
        {!reachable && device.kind === 'machine' && (
          <p className="dv-empty">Its apps show here while it's online.</p>
        )}

        {error && <p className="dv-error">{error}</p>}

        <div className="dv-actions">
          {canChangeKind && !isMe && !isPhone(device) && (
            <button
              className={`dv-act${device.kind === 'machine' ? '' : ' primary'}`}
              disabled={busy}
              onClick={() => act(() => setDeviceKind(device.id, device.kind === 'machine' ? 'desktop' : 'machine'))}
              title={device.kind === 'machine' ? 'Your other devices stop starting chats on it; it stays in your Cloud' : 'Let your other devices run chats on it'}
            >
              {device.kind === 'machine' ? 'Stop running chats for other devices' : 'Make it a machine'}
            </button>
          )}
          {confirmRemove ? (
            <>
              {/* The safe answer first: the eye, the cursor and the Tab order
                  all reach "Keep" before the press that leaves. */}
              <button className="dv-act" onClick={() => setConfirmRemove(false)}>
                Keep
              </button>
              <button
                className="dv-act"
                disabled={busy}
                onClick={() =>
                  act(async () => {
                    await removeDevice(device.id)
                    onClose()
                  })
                }
              >
                <Trash size={14} />
                {isMe ? 'Leave Cloud' : 'Remove'}
              </button>
            </>
          ) : (
            <button
              className="dv-act"
              onClick={() => setConfirmRemove(true)}
              title={
                isMe
                  ? `Take ${selfLabel(device).toLowerCase()} out of your Cloud`
                  : 'Take it out of your Cloud. The chats it ran stay on it, and its Local chats stay.'
              }
            >
              <Trash size={14} />
              {isMe ? `Remove ${selfLabel(device).toLowerCase()}` : 'Remove'}
            </button>
          )}
        </div>
        {confirmRemove && (
          <p className="dv-modal-hint">
            {isMe && isPhone(device)
              ? `This phone leaves your Cloud and deletes its ${chatCount(syncedCopies)} here. Its Local chats stay.`
              : isMe
                ? `This computer leaves your Cloud and deletes its ${chatCount(syncedCopies)} here. Its Local chats stay.`
                : sessionRows.length === 0
                  ? <><span className="pv">{device.name}</span> leaves your Cloud. Its copies of synced chats stay on it until they are deleted there. Its Local chats stay.</>
                  : <><span className="pv">{device.name}</span> leaves your Cloud. Its {chatCount(sessionRows.length)} stay on it until they are deleted there, and they stay listed here. Its Local chats stay.</>}
          </p>
        )}
      </div>
      {signInFor && (
        <SignInModal
          harnessId={signInFor}
          deviceId={isMe ? null : device.id}
          onDone={() => {
            setSignInFor(null)
            load()
          }}
        />
      )}
    </div>
  )
}

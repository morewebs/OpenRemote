// One device of the user's Cloud: what it is, the synced chats it runs
// (from this computer's copies, so they show even while it's offline),
// and, for a machine that's online, its harnesses - installed and signed
// in remotely, each by its own installer and its own login. Removing a
// device takes it out of the Cloud; it deletes the synced chats it kept,
// never its Local ones.

import { useEffect, useState } from 'react'
import { Trash, X } from '@phosphor-icons/react'
import SignInModal from './SignInModal.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { SIGNIN_HARNESSES } from './harness-manager.js'
import { useBack } from './use-back.js'
import { isPhone, kindLabel, platformLabel, selfLabel } from './cloud.js'

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
  const { sessions, thisDevice, apiFor, setDeviceKind, removeDevice, machines } = useConsole()
  const [view, setView] = useState(null)
  const [busyHarness, setBusyHarness] = useState(null)
  const [signInFor, setSignInFor] = useState(null)
  const [confirmRemove, setConfirmRemove] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const isMe = device.id === thisDevice
  // A phone has no harnesses to show or install.
  const reachable = !isPhone(device) && (isMe || (device.online && device.kind === 'machine'))
  const chats = (sessions ?? []).filter((s) => s.executor === device.id)

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

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
    } catch (err) {
      setError(err.message ?? String(err))
    }
  }
  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [device.id, reachable, machines])

  const act = async (action) => {
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (err) {
      setError(err.message ?? String(err))
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
      await load()
    } catch (err) {
      setError(err.message ?? String(err))
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
      <div className="dv-modal dv-devmodal" role="dialog" aria-modal="true" aria-label={device.name}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">{device.name}</h2>
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
          <span className="dv-label-val">{chats.length}</span>
        </div>
        <div className="dv-agents">
          {chats.length === 0 && <p className="dv-empty">No synced chats run here.</p>}
          {chats.map((s) => (
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
                  <span className="dv-agent-name">{s.title ?? harnessName(s.harness)}</span>
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
              <span>Harnesses</span>
              <span className="dv-label-val">{installed.length}</span>
            </div>
            <div className="dv-installs">
              {installed.length === 0 && <p className="dv-empty">Nothing installed yet.</p>}
              {installed.map((h) => (
                <div key={h.id} className="dv-install">
                  <div className="dv-install-body">
                    <span>{harnessName(h.id)}</span>
                    <span className="dv-agent-sub">{h.signed_in === false ? 'Not signed in' : 'Ready'}</span>
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
          </>
        )}
        {!reachable && device.kind === 'machine' && (
          <p className="dv-empty">Its harnesses show here while it's online.</p>
        )}

        {error && <p className="dv-error">{error}</p>}

        <div className="dv-actions">
          {canChangeKind && !isMe && !isPhone(device) && (
            <button
              className={`dv-act${device.kind === 'machine' ? '' : ' primary'}`}
              disabled={busy}
              onClick={() => act(() => setDeviceKind(device.id, device.kind === 'machine' ? 'desktop' : 'machine'))}
              title={device.kind === 'machine' ? 'Stop running chats for your other devices' : 'Let your other devices run chats on it'}
            >
              {device.kind === 'machine' ? 'Make it a desktop' : 'Make it a machine'}
            </button>
          )}
          {confirmRemove ? (
            <>
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
              <button className="dv-act" onClick={() => setConfirmRemove(false)}>
                Keep
              </button>
            </>
          ) : (
            <button
              className="dv-act"
              onClick={() => setConfirmRemove(true)}
              title={
                isMe
                  ? `Take ${selfLabel(device).toLowerCase()} out of your Cloud`
                  : 'Take it out of your Cloud - it deletes the synced chats it kept, never its Local ones'
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
              ? 'This phone leaves your Cloud and deletes its copies of synced chats.'
              : isMe
              ? 'This computer leaves your Cloud and deletes its copies of synced chats. Its Local chats stay.'
              : `${device.name} leaves your Cloud and deletes its copies of synced chats the next time it connects. Its Local chats stay.`}
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

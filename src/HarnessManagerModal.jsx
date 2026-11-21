import { useCallback, useEffect, useRef, useState } from 'react'
import { CaretDown, CaretRight, Desktop, DownloadSimple, SignIn, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { HarnessMark } from './brand-marks.jsx'
import { appsInventoryFor, harnessRows, selectedApp } from './harness-manager.js'
import { machinePickerItems } from './cloud.js'
import { isMobile } from './platform.js'
import PickerMenu from './PickerMenu.jsx'
import SignInModal from './SignInModal.jsx'
import AppAccounts from './AppAccounts.jsx'
import './devices.css'
import './apps-manager.css'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { errorCopy } from './error-copy.js'

export default function HarnessManagerModal({
  onClose,
  initialDeviceId = null,
  initialHarnessId = null,
  stacked = false,
}) {
  const panelRef = useRef(null)
  const appPane = useRef(null)
  const { devices, devicesAnswered, thisDevice, apiFor, installHarness, refreshCloud } = useConsole()
  const [deviceId, setDeviceId] = useState(() => initialDeviceId === thisDevice ? null : initialDeviceId)
  const [picker, setPicker] = useState(null)
  const dismissPicker = useCallback(() => setPicker(null), [])
  const [appId, setAppId] = useState(initialHarnessId)
  const [confirming, setConfirming] = useState(null)
  const [signInFor, setSignInFor] = useState(null)
  const [inventory, setInventory] = useState(null)
  const [revision, setRevision] = useState(0)
  const [accountRevision, setAccountRevision] = useState(0)
  const [installs, setInstalls] = useState(() => new Map())
  const targets = machinePickerItems(devices, thisDevice, { phone: isMobile })
  const target = targets.find((entry) => entry.id === deviceId)
  const online = Boolean(target?.online)
  const activeInstall = installs.get(deviceId)
  const canClose = !Array.from(installs.values()).some((install) => install.busy)
  const current = appsInventoryFor(inventory, deviceId, online)
  const rows = current?.view ? harnessRows(current.view, current.view.installable) : []
  const app = selectedApp(rows, appId)
  const serverInstalling = rows.some((row) => row.installing)
  const installBlocked = Boolean(activeInstall?.busy || activeInstall?.checking || serverInstalling)
  const scope = JSON.stringify([deviceId, app?.id])

  const closeOrWait = () => {
    if (picker) dismissPicker()
    else if (canClose) onClose()
  }
  useBack(true, closeOrWait)
  useDialog(panelRef, closeOrWait)

  useEffect(() => {
    if (appPane.current) appPane.current.scrollTop = 0
  }, [scope])

  useEffect(() => {
    if (!online) return
    let cancelled = false
    apiFor(deviceId).machines().then((list) => {
      if (cancelled) return
      // The daemon can know about machines that are still enrolling. Use
      // the view identifying the machine that actually answered the call.
      const view = (list ?? []).find((entry) => entry.machine?.this_machine)
      setInventory({
        deviceId,
        view,
        error: view ? null : 'This machine did not return its App list.',
      })
    }).catch((err) => {
      if (!cancelled) setInventory({ deviceId, error: errorCopy(err) })
    }).finally(() => {
      if (!cancelled && activeInstall?.checking) {
        setInstalls((prev) => new Map(prev).set(deviceId, { ...activeInstall, checking: false }))
      }
    })
    return () => { cancelled = true }
  }, [apiFor, deviceId, online, revision, activeInstall?.revision])

  useEffect(() => {
    if (!serverInstalling) return
    const timer = setTimeout(() => setRevision((n) => n + 1), 2500)
    return () => clearTimeout(timer)
  }, [deviceId, current, serverInstalling])

  const install = async (harnessId, options) => {
    const machineId = current?.view?.machine?.id
    if (!machineId || activeInstall?.busy || activeInstall?.checking || serverInstalling) return
    setConfirming(null)
    setInstalls((prev) => new Map(prev).set(deviceId, { ...activeInstall, busy: harnessId, error: null }))
    let error = null
    try {
      await installHarness(machineId, harnessId, { ...options, deviceId })
    } catch (err) {
      error = { harnessId, message: errorCopy(err) }
    } finally {
      // Even a disconnected request may have reached the installer. Refresh
      // its machine before offering another attempt.
      setInstalls((prev) => new Map(prev).set(deviceId, {
        error,
        checking: true,
        revision: (activeInstall?.revision ?? 0) + 1,
      }))
    }
  }

  const chooseMachine = (id) => {
    setDeviceId(id)
    dismissPicker()
    setConfirming(null)
  }

  const chooseApp = (id) => {
    setAppId(id)
    dismissPicker()
  }
  const togglePicker = (name) => setPicker((open) => open === name ? null : name)
  const beginSignIn = (harnessId, account = null) => {
    setSignInFor({ harnessId, account, deviceId, machineName: target?.name })
  }
  const confirmRow = rows.find((row) => row.id === confirming)
  const pending = !target && !devicesAnswered

  return (
    <div
      className={`dv-modal-backdrop dv-apps-backdrop${stacked ? ' dv-hm-stack' : ''}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) closeOrWait()
      }}
    >
      <div className="dv-modal dv-apps" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Apps">
        <div className="dv-modal-head dv-apps-head">
          <h2 className="dv-modal-title">Apps</h2>
          <div className="dv-apps-machine">
            {targets.length > 1 || !target ? (
              <button
                type="button"
                className="dv-act dv-apps-picker"
                disabled={!targets.length}
                onClick={() => togglePicker('machine')}
                aria-haspopup="listbox"
                aria-expanded={picker === 'machine'}
                data-picker="machine"
                aria-label={`Machine · ${target?.name ?? 'Choose a machine'}`}
              >
                <Desktop size={16} />
                <span className={target?.here ? '' : 'pv'}>{target?.name ?? 'Choose a machine'}</span>
                <CaretDown size={12} />
              </button>
            ) : (
              <span className="dv-apps-target">
                <Desktop size={16} />
                <span className={target.here ? '' : 'pv'}>{target.name}</span>
              </span>
            )}
            {picker === 'machine' && (
              <PickerMenu
                label="Machine"
                hideSearch={targets.length < 6}
                items={targets.map((entry) => ({
                  ...entry,
                  id: entry.id ?? 'here',
                  detail: entry.online ? undefined : 'Offline',
                  veiled: !entry.here,
                }))}
                selectedId={deviceId ?? 'here'}
                onChoose={(id) => chooseMachine(id === 'here' ? null : id)}
                onClose={dismissPicker}
              />
            )}
          </div>
          <button
            type="button"
            className="dv-modal-close"
            disabled={!canClose}
            onClick={closeOrWait}
            title={canClose ? 'Close' : 'An install is still running'}
            aria-label="Close Apps"
          >
            <X size={14} weight="bold" />
          </button>
        </div>

        {!target || !online || current?.error ? (
          <div className="dv-apps-message">
            <p role={current?.error ? 'alert' : 'status'}>
              {pending ? 'Finding your machines…' : current?.error ?? (target ? 'This machine is offline.' : 'This machine is unavailable.')}
            </p>
            {!pending && (target || targets.length === 0) && (
              <button type="button" className="dv-act" onClick={() => {
                if (online) setRevision((n) => n + 1)
                else refreshCloud?.().catch(() => {})
              }}>Retry</button>
            )}
          </div>
        ) : !current ? (
          <p className="dv-apps-message" role="status">Loading Apps…</p>
        ) : !app ? (
          <p className="dv-apps-message" role="status">This machine has no Apps to show.</p>
        ) : (
          <div className="dv-apps-layout">
            <nav className="dv-apps-nav" aria-label="Apps on this machine">
              {rows.map((row) => {
                const status = activeInstall?.busy === row.id ? 'Installing…' : row.statusLabel
                return (
                  <button
                    key={row.id}
                    type="button"
                    className="dv-apps-nav-item"
                    aria-current={row.id === app.id ? 'true' : undefined}
                    onClick={() => chooseApp(row.id)}
                  >
                    <HarnessMark harness={row.id} size={17} />
                    <span className="dv-apps-nav-copy">
                      <span>{row.name}</span>
                      {!row.ready && row.action !== 'accounts' && <small>{status}</small>}
                    </span>
                  </button>
                )
              })}
            </nav>
            <div className="dv-apps-pane">
              <div className="dv-apps-app-head">
                <span className="dv-apps-app-mark"><HarnessMark harness={app.id} size={24} /></span>
                <div className="dv-apps-app-title">
                  <h3>{app.name}</h3>
                  <button
                    type="button"
                    className="dv-apps-app-picker"
                    aria-label={`Choose app · ${app.name}`}
                    aria-haspopup="listbox"
                    aria-expanded={picker === 'app'}
                    data-picker="app"
                    onClick={() => togglePicker('app')}
                  >
                    <span>{app.name}</span>
                    <CaretDown size={13} />
                  </button>
                  <AppStatus row={app} installing={activeInstall?.busy === app.id || app.installing} />
                </div>
                {picker === 'app' && (
                  <PickerMenu
                    label="Choose app"
                    items={rows.map((row) => ({ id: row.id, name: row.name, detail: row.statusLabel }))}
                    selectedId={app.id}
                    hideSearch={rows.length < 9}
                    renderIcon={(row) => <HarnessMark harness={row.id} size={16} />}
                    onChoose={chooseApp}
                    onClose={dismissPicker}
                  />
                )}
              </div>
              <div className="dv-apps-pane-scroll" ref={appPane}>
                <AppDetails
                  key={scope}
                  row={app}
                  installBlocked={installBlocked}
                  installError={activeInstall?.error?.harnessId === app.id ? activeInstall.error.message : null}
                  onInstall={() => app.action === 'install-runtime' ? setConfirming(app.id) : install(app.id)}
                  onSignIn={(account) => beginSignIn(app.id, account)}
                  deviceId={deviceId}
                  apiFor={apiFor}
                  accountRevision={accountRevision}
                />
              </div>
            </div>
          </div>
        )}
      </div>

      {confirmRow && (
        <RuntimeConfirm
          name={confirmRow.name}
          machineName={target?.name}
          onCancel={() => setConfirming(null)}
          onConfirm={() => install(confirmRow.id, { withRuntime: true })}
        />
      )}
      {signInFor && signInFor.deviceId === deviceId && (
        <SignInModal
          harnessId={signInFor.harnessId}
          deviceId={signInFor.deviceId}
          machineName={signInFor.machineName}
          accountId={signInFor.account?.id}
          accountLabel={signInFor.account?.label}
          onDone={() => {
            setSignInFor(null)
            setRevision((n) => n + 1)
            setAccountRevision((n) => n + 1)
          }}
        />
      )}
    </div>
  )
}

function AppStatus({ row, installing }) {
  const managesAccounts = row.action === 'accounts' && !isMobile
  const ready = !installing && (managesAccounts || row.ready)
  const status = installing ? 'Installing…' : managesAccounts ? 'Installed' : row.defaultSignin ?? row.statusLabel
  return (
    <p className={`dv-apps-status${ready ? ' ready' : ''}`} role="status">
      <span aria-hidden="true" />{status}
    </p>
  )
}

function AppDetails({ row, installBlocked, installError, onInstall, onSignIn, deviceId, apiFor, accountRevision }) {
  const managesAccounts = row.action === 'accounts' && !isMobile
  const installable = row.action === 'install' || row.action === 'install-runtime'
  const signIn = !managesAccounts && row.canSignIn
  const note = row.detailKind === 'note' ? row.detail : null
  const probe = row.detailKind === 'mono' ? row.detail : null
  return (
    <div className="dv-apps-detail">
      {managesAccounts ? (
        <AppAccounts row={row} deviceId={deviceId} apiFor={apiFor} onSignIn={onSignIn} revision={accountRevision} />
      ) : (
        <div className="dv-apps-overview">
          {note && <p>{note}</p>}
          {installError && installable && (
            <div className="dv-error dv-hm-error" role="alert"><pre>{installError}</pre></div>
          )}
          {installable && (
            <button type="button" className="dv-act dv-apps-primary" disabled={installBlocked} onClick={onInstall}>
              <DownloadSimple size={15} />Install {row.name}
            </button>
          )}
          {signIn && (
            <button type="button" className="dv-act dv-apps-primary" onClick={() => onSignIn(null)}>
              <SignIn size={15} />Sign in to {row.name}
            </button>
          )}
          {row.installed && !isMobile && !note && !signIn && <p>Multiple accounts are unavailable for this app on this machine.</p>}
        </div>
      )}
      {(probe || row.installCommand || row.caps) && (
        <details className="dv-install-more">
          <summary><CaretRight size={12} aria-hidden="true" />{row.installCommand ? 'Show the command' : 'App details'}</summary>
          {probe && <code className="dv-apps-code pv">{probe}</code>}
          {row.installCommand && <code className="dv-apps-code">{row.installCommand}</code>}
          {row.caps && (
            <div className="dv-hm-caps">
              {row.caps.map((cap) => (
                <span
                  key={cap.key}
                  className={`dv-hm-cap${cap.on ? '' : ' off'}`}
                  title={`${row.name} ${cap.on ? 'honors' : 'does not honor'} ${cap.label.toLowerCase()}`}
                >{cap.label}</span>
              ))}
            </div>
          )}
        </details>
      )}
    </div>
  )
}

function RuntimeConfirm({ name, machineName, onCancel, onConfirm }) {
  const panelRef = useRef(null)
  useBack(true, onCancel)
  useDialog(panelRef, onCancel)
  return (
    <div className="dv-modal-backdrop dv-hm-stack" onMouseDown={(e) => {
      if (e.target === e.currentTarget) onCancel()
    }}>
      <div className="dv-modal dv-hm-confirm" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={`Install ${name}`}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Install {name}</h2>
          <button type="button" className="dv-modal-close" onClick={onCancel} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          {name} needs Node.js 22. OpenRemote will install both on <span className="pv">{machineName}</span>.
        </p>
        <div className="dv-actions">
          <button type="button" className="dv-act" autoFocus onClick={onCancel}>Cancel</button>
          <button type="button" className="dv-act primary" onClick={onConfirm}>Install {name}</button>
        </div>
      </div>
    </div>
  )
}

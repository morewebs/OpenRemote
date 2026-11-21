import { useCallback, useEffect, useRef, useState } from 'react'
import { Desktop, DotsThree, PencilSimple, Plus, SignIn, SignOut, User, X } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useDialog } from './dialog-focus.js'
import { errorCopy } from './error-copy.js'
import { useAppAccounts } from './use-app-accounts.js'
import { useBack } from './use-back.js'

export default function AppAccounts({ row, deviceId, apiFor, onSignIn, revision }) {
  const { accounts, error: loadError, loading, refresh } = useAppAccounts(apiFor, deviceId, row.id, {
    signInRevision: revision,
    keepListWhileRefreshing: true,
  })
  const [menuId, setMenuId] = useState(null)
  const [editor, setEditor] = useState(null)
  const menuTrigger = useRef(null)
  const closeMenu = useCallback(() => setMenuId(null), [])
  const mounted = useRef(false)

  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false }
  }, [])

  const save = async (label) => {
    const api = apiFor(deviceId)
    let created
    if (editor.kind === 'add') {
      const answer = await api.createAppAccount(row.id, label)
      created = answer?.result ?? answer
      if (!created?.id) throw new Error('This machine did not return the new account.')
    } else {
      const options = editor.kind === 'rename' ? { label } : { disconnect: true }
      await api.updateAppAccount(row.id, editor.account.id, options)
    }
    if (!mounted.current) return
    setEditor(null)
    refresh()
    if (created) onSignIn(created)
  }

  return (
    <section className="dv-apps-accounts" aria-label={`${row.name} accounts`}>
      <div className="dv-apps-account-head">
        <h4>Accounts</h4>
        <button
          type="button"
          className="dv-act dv-apps-primary"
          onClick={() => setEditor({ kind: 'add' })}
          disabled={loading || Boolean(loadError)}
        >
          <Plus size={15} weight="bold" />Add account
        </button>
      </div>
      <ul className="dv-apps-account-list">
        <AccountRow label="Existing sign-in" status={row.defaultSignin} ready={row.ready} existing>
          {row.canSignIn && <button type="button" className="dv-act" onClick={() => onSignIn(null)}>Sign in</button>}
        </AccountRow>
        {accounts.map((account) => {
          const ready = account.status === 'ready'
          return (
            <AccountRow key={account.id} label={account.label} status={ready ? 'Ready' : 'Needs sign-in'} ready={ready}>
              {!ready && <button type="button" className="dv-act" onClick={() => onSignIn(account)}>Reconnect</button>}
              <button
                type="button"
                className="dv-apps-icon-button"
                aria-label={`Actions for ${account.label}`}
                title="Account actions"
                aria-haspopup="menu"
                aria-expanded={menuId === account.id}
                data-picker="account-actions"
                onClick={(event) => {
                  menuTrigger.current = event.currentTarget
                  setMenuId((open) => open === account.id ? null : account.id)
                }}
              >
                <DotsThree size={19} weight="bold" />
              </button>
              {menuId === account.id && (
                <PickerMenu
                  label="Account actions"
                  presentation="actions"
                  hideSearch
                  items={[
                    { id: 'rename', name: 'Rename', icon: PencilSimple },
                    ...(ready ? [
                      { id: 'reconnect', name: 'Reconnect', icon: SignIn },
                      { id: 'disconnect', name: 'Disconnect…', icon: SignOut },
                    ] : []),
                  ]}
                  renderIcon={({ icon: Icon }) => <Icon size={16} />}
                  onChoose={(kind) => {
                    menuTrigger.current?.focus()
                    closeMenu()
                    if (kind === 'reconnect') onSignIn(account)
                    else setEditor({ kind, account })
                  }}
                  onClose={closeMenu}
                />
              )}
            </AccountRow>
          )
        })}
      </ul>
      {loading && <p className="dv-apps-account-status" role="status">Loading accounts…</p>}
      {loadError && (
        <div className="dv-apps-account-error">
          <p className="dv-error" role="alert">{errorCopy(loadError)}</p>
          <button type="button" className="dv-act" onClick={refresh}>Retry</button>
        </div>
      )}
      {editor && <AccountEditor editor={editor} appName={row.name} onSave={save} onClose={() => setEditor(null)} />}
    </section>
  )
}

function AccountRow({ label, status, ready, existing, children }) {
  return (
    <li className="dv-apps-account-row">
      <span className="dv-apps-account-icon" aria-hidden="true">{existing ? <Desktop size={17} /> : <User size={17} />}</span>
      <div className="dv-apps-account-name">
        <span className={existing ? undefined : 'pv'}>{label}</span>
        <span className={`dv-apps-account-status${ready ? ' ready' : ''}`}>{status}</span>
      </div>
      <div className="dv-apps-account-controls">{children}</div>
    </li>
  )
}

function AccountEditor({ editor, appName, onSave, onClose }) {
  const { kind, account } = editor
  const disconnecting = kind === 'disconnect'
  const [label, setLabel] = useState(account?.label ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const panelRef = useRef(null)
  const initialFocus = useRef(null)
  const close = () => { if (!busy) onClose() }
  useBack(true, close)
  useDialog(panelRef, close)
  useEffect(() => initialFocus.current?.focus(), [])
  const title = kind === 'add' ? `Add ${appName} account` : kind === 'rename' ? 'Rename account' : 'Disconnect account'
  const action = kind === 'add' ? 'Continue to sign-in' : kind === 'rename' ? 'Save name' : 'Disconnect'

  const submit = async (event) => {
    event.preventDefault()
    if (busy || (!disconnecting && !label.trim())) return
    setBusy(true)
    setError(null)
    try {
      await onSave(label.trim())
    } catch (err) {
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  return (
    <div className="dv-modal-backdrop dv-hm-stack dv-apps-editor-backdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget) close()
    }}>
      <form className="dv-modal dv-apps-editor" role="dialog" aria-modal="true" aria-label={title} ref={panelRef} tabIndex={-1} onSubmit={submit}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">{title}</h2>
          <button type="button" className="dv-modal-close" aria-label="Close" onClick={close} disabled={busy}><X size={14} /></button>
        </div>
        {disconnecting ? (
          <p className="dv-modal-hint">
            Disconnect <b className="pv">{account.label}</b> from this machine? Its chats will need this account reconnected before they can resume.
          </p>
        ) : (
          <label className="dv-host">
            Account name
            <input ref={initialFocus} value={label} onChange={(event) => setLabel(event.target.value)} disabled={busy} />
          </label>
        )}
        {error && <p className="dv-error" role="alert">{error}</p>}
        <div className="dv-actions">
          <button type="button" className="dv-act" ref={disconnecting ? initialFocus : null} onClick={close} disabled={busy}>Cancel</button>
          <button type="submit" className={`dv-act ${disconnecting ? 'dv-apps-danger' : 'dv-apps-primary'}`} disabled={busy || (!disconnecting && !label.trim())}>
            {busy ? 'Saving…' : action}
          </button>
        </div>
      </form>
    </div>
  )
}

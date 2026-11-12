// Account: the moreweb account this browser is a device of, and its machines.

import { useConsole } from '../state/console.jsx'
import { kindLabel, platformLabel } from '../cloud.js'
import '../devices.css'
import '../panels.css'

export default function WebAccountView({ onSignOut }) {
  const { cloud, devices, thisDevice } = useConsole()
  const list = Array.isArray(devices) ? devices : []
  const me = list.find((device) => device.id === thisDevice) ?? cloud?.device ?? null
  const machines = list.filter((device) => device.kind === 'machine')
  const online = machines.filter((device) => device.online).length
  return (
    <div className="panel">
      <header className="dv-head">
        <div>
          <h1 className="dv-title">Account</h1>
          <p className="dv-meta">{cloud?.account?.email ?? 'Signed in'}</p>
        </div>
      </header>
      <h2 className="pn-section">This browser</h2>
      <p className="pn-lead">
        This browser is one of your account's devices. Your chats stay on your machines; this page shows them and sends what you type.
      </p>
      <div className="pn-list pn-list--tight">
        {me && (
          <div className="pn-row">
            <div>
              <div className="pn-name pv">{me.name}</div>
              <div className="pn-detail">
                {[platformLabel(me.platform), kindLabel(me)].filter(Boolean).join(' · ')}
              </div>
            </div>
          </div>
        )}
        <div className="pn-row">
          <div>
            <div className="pn-name">
              {list.length} {list.length === 1 ? 'device' : 'devices'}
            </div>
            <div className="pn-detail">
              {machines.length} {machines.length === 1 ? 'machine' : 'machines'} · {online} online
            </div>
          </div>
          <button type="button" className="pn-btn" onClick={onSignOut}>
            Sign out
          </button>
        </div>
      </div>
    </div>
  )
}

// Machines: every computer in the user's Cloud. A machine runs chats for
// the other devices; a desktop that isn't one only syncs. This computer
// comes first, with the switch that makes it a machine.

import { useState } from 'react'
import { Desktop, HardDrives, Plus } from '@phosphor-icons/react'
import MachineModal from './MachineModal.jsx'
import AddMachineModal from './AddMachineModal.jsx'
import { useConsole } from './state/console.jsx'
import './devices.css'

const PLATFORM_LABEL = { windows: 'Windows', macos: 'macOS', linux: 'Linux' }

function detail(device) {
  const platform = PLATFORM_LABEL[device.platform] ?? device.platform
  const kind = device.kind === 'machine' ? 'Machine' : 'Desktop'
  return [platform, kind, device.app_version].filter(Boolean).join(' · ')
}

export default function MachinesView({ onOpenChat }) {
  const { devices, thisDevice, setDeviceKind } = useConsole()
  const [adding, setAdding] = useState(false)
  const [openId, setOpenId] = useState(null)
  const [error, setError] = useState(null)
  const list = [...(devices ?? [])].sort(
    (a, b) => Number(b.id === thisDevice) - Number(a.id === thisDevice) || a.name.localeCompare(b.name),
  )
  const me = list.find((d) => d.id === thisDevice) ?? null
  const onlineCount = list.filter((d) => d.online).length
  const open = list.find((d) => d.id === openId) ?? null

  const toggleMine = async () => {
    setError(null)
    try {
      await setDeviceKind(me.id, me.kind === 'machine' ? 'desktop' : 'machine')
    } catch (err) {
      setError(err.message ?? String(err))
    }
  }

  return (
    <div className="devices">
      <header className="dv-head">
        <h1 className="dv-title">Machines</h1>
        <p className="dv-meta">
          {list.length} {list.length === 1 ? 'device' : 'devices'} · {onlineCount} online
        </p>
      </header>
      {me && me.created_via !== 'enrollment' && (
        <div className="dv-self">
          <div>
            <div className="dv-self-name">
              {me.kind === 'machine' ? 'This computer is a machine' : 'Make this computer a machine'}
            </div>
            <div className="dv-self-detail">
              {me.kind === 'machine'
                ? 'Your other devices can start and continue chats here while OpenRemote is open.'
                : 'Let your other devices start and continue chats here while OpenRemote is open.'}
            </div>
          </div>
          <button type="button" className={`dv-act${me.kind === 'machine' ? '' : ' primary'}`} onClick={toggleMine}>
            {me.kind === 'machine' ? 'Stop' : 'Make it a machine'}
          </button>
        </div>
      )}
      {error && <p className="dv-error">{error}</p>}
      <div className="dv-grid">
        {list.map((device) => (
          <button
            key={device.id}
            className="dv-card"
            onClick={() => setOpenId(device.id)}
            title={device.name}
          >
            <span className="dv-icon">
              {device.kind === 'machine' ? <HardDrives size={19} weight="light" /> : <Desktop size={19} weight="light" />}
            </span>
            <span className="dv-name">
              {device.name}
              {device.id === thisDevice && <span className="dv-badge">This computer</span>}
            </span>
            <span className="dv-detail">{detail(device)}</span>
            <span className="dv-status">
              {device.problem === 'key_changed' ? (
                <>
                  <span className="dv-dot dv-dot--offline" />
                  Key changed
                </>
              ) : (
                <>
                  <span className={`dv-dot dv-dot--${device.online ? 'online' : 'offline'}`} />
                  {device.online ? 'Online' : 'Offline'}
                </>
              )}
            </span>
          </button>
        ))}
        <button className="dv-add" title="Add a machine" onClick={() => setAdding(true)}>
          <span className="dv-add-plus">
            <Plus size={19} weight="light" />
          </span>
          <span className="dv-add-label">Add a machine</span>
        </button>
      </div>
      {adding && <AddMachineModal onClose={() => setAdding(false)} />}
      {open && <MachineModal device={open} onOpenChat={onOpenChat} onClose={() => setOpenId(null)} />}
    </div>
  )
}

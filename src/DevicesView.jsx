import { useState } from 'react'
import { Plus } from '@phosphor-icons/react'
import { deviceIcon } from './catalog.js'
import AddDeviceModal from './AddDeviceModal.jsx'
import DeviceModal from './DeviceModal.jsx'
import './devices.css'

const STATUS_LABEL = { online: 'Online', offline: 'Offline', waiting: 'Waiting' }

export default function DevicesView({ devices, signedIn, onOpenChat, onAdd, onTogglePower, onInstall }) {
  const [adding, setAdding] = useState(false)
  const [openId, setOpenId] = useState(null)
  const onlineCount = devices.filter((d) => d.status === 'online').length
  const waitingCount = devices.filter((d) => d.status === 'waiting').length
  const device = devices.find((d) => d.id === openId)

  return (
    <div className="devices">
      <header className="dv-head">
        <h1 className="dv-title">Machines</h1>
        <p className="dv-meta">
          {devices.length} {devices.length === 1 ? 'machine' : 'machines'} · {onlineCount} online
          {waitingCount > 0 ? ` · ${waitingCount} waiting` : ''}
        </p>
      </header>
      <div className="dv-grid">
        {devices.map((d) => {
          const Icon = deviceIcon(d.kind)
          return (
            <button
              key={d.id}
              className={`dv-card${d.status === 'offline' ? ' dv-off' : ''}`}
              onClick={() => setOpenId(d.id)}
              title={d.name}
            >
              <span className="dv-icon">
                <Icon size={19} weight="light" />
              </span>
              <span className="dv-name">{d.name}</span>
              <span className="dv-detail">{d.detail}</span>
              <span className="dv-status">
                <span className={`dv-dot dv-dot--${d.status}`} />
                {STATUS_LABEL[d.status] ?? d.status}
              </span>
            </button>
          )
        })}
        <button className="dv-add" title="Add a device" onClick={() => setAdding(true)}>
          <span className="dv-add-plus">
            <Plus size={19} weight="light" />
          </span>
          <span className="dv-add-label">Add a device</span>
        </button>
      </div>
      {adding && (
        <AddDeviceModal
          devices={devices}
          onClose={() => setAdding(false)}
          onAdd={(input) => {
            onAdd(input)
            setAdding(false)
          }}
        />
      )}
      {device && (
        <DeviceModal
          device={device}
          signedIn={signedIn}
          onOpenChat={onOpenChat}
          onClose={() => setOpenId(null)}
          onTogglePower={onTogglePower}
          onInstall={onInstall}
        />
      )}
    </div>
  )
}

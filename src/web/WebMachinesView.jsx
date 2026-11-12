// Machines: the account's computers, read-only from the browser. Picking one
// makes it the target for New chat and the chat list it copies.

import { HardDrives } from '@phosphor-icons/react'
import { useConsole } from '../state/console.jsx'
import { platformLabel } from '../cloud.js'
import '../devices.css'

export default function WebMachinesView({ onPick }) {
  const { devices, devicesAnswered } = useConsole()
  const machines = (devices ?? []).filter((device) => device.kind === 'machine')
  const online = machines.filter((device) => device.online).length
  return (
    <div className="devices">
      <header className="dv-head">
        <h1 className="dv-title">Machines</h1>
        <p className="dv-meta">
          {devicesAnswered
            ? `${machines.length} ${machines.length === 1 ? 'machine' : 'machines'} · ${online} online`
            : 'Checking your devices…'}
        </p>
      </header>
      <p className="dv-meta">
        Your chats run here, on your own computers. Pick one to start or continue a chat.
      </p>
      <div className="dv-grid">
        {machines.map((device) => (
          <button
            key={device.id}
            type="button"
            className="dv-card"
            onClick={() => onPick(device.id)}
            title={device.name}
          >
            <span className="dv-icon">
              <HardDrives size={20} weight="duotone" />
            </span>
            <span className="dv-name">
              <span className="pv">{device.name}</span>
            </span>
            <span className="dv-detail">
              {[platformLabel(device.platform), device.app_version ? `v${device.app_version}` : null]
                .filter(Boolean)
                .join(' · ')}
            </span>
            <span className="dv-status">
              <span className={`dv-dot dv-dot--${device.online ? 'online' : 'offline'}`} />
              {device.online ? 'Online' : 'Offline'}
            </span>
          </button>
        ))}
        {devicesAnswered && machines.length === 0 && (
          <p className="dv-meta">
            No machines yet. On one of your computers, open OpenRemote and choose Machines, then
            Make this computer a machine.
          </p>
        )}
      </div>
    </div>
  )
}

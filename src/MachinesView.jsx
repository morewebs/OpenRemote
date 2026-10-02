// Machines: every computer that can run agent chats. This machine is
// real from the first boot (its inventory, its sessions, its presence);
// added machines wait until their agent checks in.

import { useState } from 'react'
import { Desktop, Plus } from '@phosphor-icons/react'
import MachineModal from './MachineModal.jsx'
import AddMachineModal from './AddMachineModal.jsx'
import { useConsole } from './state/console.jsx'
import './devices.css'

const STATUS_LABEL = { online: 'Online', offline: 'Offline', waiting: 'Waiting' }
const PLATFORM_LABEL = { windows: 'Windows', macos: 'macOS', linux: 'Linux' }

function machineDetail(view) {
  const { machine, sessions } = view
  if (machine.status === 'waiting') return 'Waiting for the agent'
  const count = sessions?.length ?? 0
  return `${PLATFORM_LABEL[machine.platform] ?? machine.platform} · ${count} ${count === 1 ? 'session' : 'sessions'}`
}

export default function MachinesView({ onOpenChat }) {
  const { machines } = useConsole()
  const [adding, setAdding] = useState(false)
  const [openId, setOpenId] = useState(null)
  const list = machines ?? []
  const onlineCount = list.filter((m) => m.machine.status === 'online').length
  const waitingCount = list.filter((m) => m.machine.status === 'waiting').length
  const open = list.find((m) => m.machine.id === openId) ?? null

  return (
    <div className="devices">
      <header className="dv-head">
        <h1 className="dv-title">Machines</h1>
        <p className="dv-meta">
          {list.length} {list.length === 1 ? 'machine' : 'machines'} · {onlineCount} online
          {waitingCount > 0 ? ` · ${waitingCount} waiting` : ''}
        </p>
      </header>
      <div className="dv-grid">
        {list.map((view) => (
          <button
            key={view.machine.id}
            className="dv-card"
            onClick={() => setOpenId(view.machine.id)}
            title={view.machine.name}
          >
            <span className="dv-icon">
              <Desktop size={19} weight="light" />
            </span>
            <span className="dv-name">{view.machine.name}</span>
            <span className="dv-detail">{machineDetail(view)}</span>
            <span className="dv-status">
              <span className={`dv-dot dv-dot--${view.machine.status}`} />
              {STATUS_LABEL[view.machine.status] ?? view.machine.status}
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
      {open && <MachineModal view={open} onOpenChat={onOpenChat} onClose={() => setOpenId(null)} />}
    </div>
  )
}

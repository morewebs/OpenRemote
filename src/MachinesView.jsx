// Machines: every computer in the user's Cloud. A machine runs chats for
// the other devices; a desktop that isn't one only syncs. This computer
// comes first, with the switch that makes it a machine.

import { useEffect, useState } from 'react'
import { Desktop, DeviceMobile, HardDrives, Plus } from '@phosphor-icons/react'
import MachineModal from './MachineModal.jsx'
import AddMachineModal from './AddMachineModal.jsx'
import { useConsole } from './state/console.jsx'
import { usePrivacyMode } from './privacy.js'
import { isPhone, kindLabel, platformLabel, selfLabel } from './cloud.js'
import { list as asList, text } from './shape.js'
import './devices.css'
import { errorCopy } from './error-copy.js'

function detail(device) {
  // The card prints what the list sorts, so it reads through the same
  // coercions: a malformed row must not throw inside the pane it opens.
  // The whole record goes to kindLabel with only its kind repaired: which kind
  // a device is depends on its platform too, so handing over the kind alone
  // would label a paired phone as a desktop.
  const platform = platformLabel(text(device?.platform))
  const kind = kindLabel({ ...(device ?? {}), kind: text(device?.kind) })
  return [platform, kind, text(device?.app_version)].filter(Boolean).join(' · ')
}

export default function MachinesView({ onOpenChat, openDevice = null }) {
  const privacy = usePrivacyMode()
  const { devices, devicesAnswered, thisDevice, cloud, setDeviceKind } = useConsole()
  const [adding, setAdding] = useState(false)
  const [openId, setOpenId] = useState(null)
  // The command palette's machine row lands here with that machine open.
  useEffect(() => {
    if (openDevice) setOpenId(openDevice.id)
  }, [openDevice?.nonce])
  const [error, setError] = useState(null)
  // The same rule the machine picker follows: a device row is daemon data,
  // so the list can be the wrong shape and a name can be missing. One bad row
  // must not throw the view that lists every machine.
  const list = asList(devices).sort(
    (a, b) => Number(b.id === thisDevice) - Number(a.id === thisDevice) || text(a.name).localeCompare(text(b.name)),
  )
  // This device's own row. The account view answers even while the device list
  // has not, and it carries the same id, name, kind and platform, so the self
  // row is not withheld from a person just because the list is still coming.
  // Unknown is not empty: an unanswered list is not an account with no devices,
  // and it must not hide the way to enrol this computer.
  const me = list.find((d) => d.id === thisDevice) ?? cloud?.device ?? null
  const onlineCount = list.filter((d) => d.online).length
  const open = list.find((d) => d.id === openId) ?? null

  // One flip at a time: the daemon's answer takes a moment, and a second
  // click while the first is out reads the same stale kind and sends the same
  // request twice, which is how a toggle ends up flipped back.
  const [busy, setBusy] = useState(false)
  const toggleMine = async () => {
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      await setDeviceKind(me.id, me.kind === 'machine' ? 'desktop' : 'machine')
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="devices">
      <header className="dv-head">
        <h1 className="dv-title">Machines</h1>
        <p className="dv-meta">
          {devicesAnswered
            ? `${list.length} ${list.length === 1 ? 'device' : 'devices'} · ${onlineCount} online`
            : 'Checking your devices…'}
        </p>
      </header>
      {/* A phone runs no chats, so it is never offered as a machine. */}
      {me && me.created_via !== 'enrollment' && !isPhone(me) && (
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
          <button
            type="button"
            className={`dv-act${me.kind === 'machine' ? '' : ' primary'}`}
            disabled={busy}
            onClick={toggleMine}
          >
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
            title={privacy ? undefined : device.name}
          >
            <span className="dv-icon">
              {isPhone(device) ? (
                <DeviceMobile size={20} weight="duotone" />
              ) : device.kind === 'machine' ? (
                <HardDrives size={20} weight="duotone" />
              ) : (
                <Desktop size={20} weight="duotone" />
              )}
            </span>
            <span className="dv-name">
              <span className="pv">{device.name}</span>
              {device.id === thisDevice && <span className="dv-badge">{selfLabel(device)}</span>}
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
            <Plus size={20} weight="bold" />
          </span>
          <span className="dv-add-label">Add a machine</span>
        </button>
      </div>
      {adding && <AddMachineModal onClose={() => setAdding(false)} />}
      {open && <MachineModal device={open} onOpenChat={onOpenChat} onClose={() => setOpenId(null)} />}
    </div>
  )
}

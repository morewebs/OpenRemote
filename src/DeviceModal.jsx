import { useEffect } from 'react'
import { Power, X } from '@phosphor-icons/react'
import { HarnessIcon } from './BrandIcon.jsx'
import { HARNESSES, deviceIcon, harnessName } from './catalog.js'

const SLICES = 48

const fmt = (d) =>
  `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`

function presence(device) {
  if (device.status === 'waiting') return 'Waiting for the agent'
  if (device.status === 'offline') return `Last seen ${device.lastSeen}`
  return device.since ? `Online · ${device.since}` : 'Online'
}

export default function DeviceModal({ device, signedIn, onOpenChat, onClose, onTogglePower, onInstall }) {
  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const Icon = deviceIcon(device.kind)
  const agents = device.agents ?? []
  const installed = device.harnesses ?? []
  const down = new Set(device.downSlices ?? [])
  const pct = (((SLICES - down.size) / SLICES) * 100).toFixed(1)
  const now = Date.now()
  const offline = device.status === 'offline'
  const waiting = device.status === 'waiting'
  const missing = (signedIn ?? [])
    .filter((id) => !installed.includes(id))
    .map((id) => HARNESSES.find((h) => h.id === id))
    .filter(Boolean)

  const openChat = (chatId) => {
    if (!chatId) return
    onClose()
    onOpenChat?.(chatId)
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className={`dv-modal dv-devmodal${offline ? ' off' : ''}`}
        role="dialog"
        aria-modal="true"
        aria-label={device.name}
      >
        <div className="dv-modal-head">
          <div className="dv-dev-id">
            <span className="dv-icon">
              <Icon size={19} weight="light" />
            </span>
            <div>
              <h2 className="dv-modal-title">{device.name}</h2>
              <div className="dv-presence">
                <span className={`dv-pdot dv-pdot--${device.status}`} />
                {presence(device)}
              </div>
              <div className="dv-spec">{device.spec}</div>
            </div>
          </div>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>

        {waiting ? (
          <p className="dv-waitnote">
            Run the install command on this machine. It comes online when the agent checks in. Nothing can be installed until then.
          </p>
        ) : (
          <>
            <div className="dv-label-row">
              <span>Uptime · last 24h</span>
              <span className="dv-label-val dv-label-val--up">{pct}%</span>
            </div>
            <div className="dv-band">
              {Array.from({ length: SLICES }, (_, i) => {
                const up = !down.has(i)
                const start = new Date(now - (SLICES - i) * 30 * 60000)
                const end = new Date(now - (SLICES - 1 - i) * 30 * 60000)
                return (
                  <span
                    key={i}
                    className={up ? 'dv-slice' : 'dv-slice down'}
                    title={`${fmt(start)} – ${fmt(end)} · ${up ? 'Up' : 'Down'}`}
                  />
                )
              })}
            </div>
          </>
        )}

        <div className="dv-label-row">
          <span>Sessions</span>
          <span className="dv-label-val">{agents.length}</span>
        </div>
        <div className="dv-agents">
          {agents.length === 0 && <p className="dv-empty">No sessions on this machine.</p>}
          {agents.map((a, i) => {
            const h = HARNESSES.find((x) => x.id === a.harness)
            return (
              <button
                key={`${a.chatId ?? 'none'}-${a.harness}-${i}`}
                className="dv-agent"
                onClick={() => openChat(a.chatId)}
                disabled={!a.chatId}
              >
                <HarnessIcon harness={h} size={15} />
                <div className="dv-agent-body">
                  <div className="dv-agent-top">
                    <span className="dv-agent-name">{harnessName(a.harness)}</span>
                    <span className={`dv-agent-dot${a.status === 'active' ? '' : ' idle'}`} />
                  </div>
                  <div className="dv-agent-sub">
                    {a.model}
                    {a.activity ? ` · ${a.activity}` : ''}
                  </div>
                </div>
              </button>
            )
          })}
        </div>

        <div className="dv-label-row">
          <span>Harnesses on this machine</span>
          <span className="dv-label-val">{installed.length}</span>
        </div>
        <div className="dv-harnesses">
          {installed.length === 0 && <p className="dv-empty">Nothing installed yet.</p>}
          {installed.map((id) => (
            <span key={id} className="dv-chip">{harnessName(id)}</span>
          ))}
        </div>
        {device.status === 'online' && missing.length > 0 && (
          <div className="dv-installs">
            {missing.map((h) => (
              <div key={h.id} className="dv-install">
                <span>{h.name}</span>
                <button type="button" className="dv-act" onClick={() => onInstall(device.id, h.id)}>
                  Install
                </button>
              </div>
            ))}
          </div>
        )}
        {offline && (
          <p className="dv-waitnote">Reconnect this machine before installing a harness on it.</p>
        )}

        {device.status !== 'waiting' && (
          <div className="dv-actions">
            <button
              className="dv-act"
              title={offline ? 'Mark this machine reachable again' : 'Mark this machine unreachable'}
              onClick={() => onTogglePower(device.id, offline)}
            >
              <Power size={14} />
              {offline ? 'Reconnect' : 'Disconnect'}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}

import { useState } from 'react'
import { PLUGINS } from './library.js'
import { deviceIcon } from './catalog.js'
import AddPluginModal from './AddPluginModal.jsx'
import './devices.css'
import './panels.css'

function deviceName(devices, id) {
  return devices.find((d) => d.id === id)?.name ?? id
}

export default function PluginsView({ world, onInstall, onAdd, onRemove, onKey, onEnabled }) {
  const [query, setQuery] = useState('')
  const [picking, setPicking] = useState(null)
  const [adding, setAdding] = useState(false)
  const q = query.trim().toLowerCase()
  const catalog = PLUGINS.filter((p) => !q || p.name.toLowerCase().includes(q))

  return (
    <div className="panel">
      <header className="dv-head pn-head">
        <div>
          <h1 className="dv-title">Plugins</h1>
          <p className="dv-meta">Tools a session can call. Each one is installed on a machine, and a key never leaves that machine.</p>
        </div>
        <button type="button" className="dv-act" onClick={() => setAdding(true)}>Add plugin</button>
      </header>

      <h2 className="pn-section">On your machines</h2>
      {world.plugins.length === 0 && (
        <p className="pn-empty">Nothing on your machines yet. Install one below, or add your own.</p>
      )}
      <div className="pn-list pn-list--wide">
        {world.plugins.map((p) => {
          const host = deviceName(world.devices, p.deviceId)
          const status = p.status === 'running' ? 'Running' : p.status === 'needs-key' ? 'Needs a key' : 'Off'
          return (
            <div key={p.id} className="pn-row pn-row--stack">
              <div className="pn-row-main">
                <div>
                  <div className="pn-name">{p.name}</div>
                  <div className="pn-detail">{p.detail} · {host}</div>
                </div>
                <div className="pn-actions">
                  <span className={`pn-state pn-state--${p.status}`}>{status}</span>
                  {p.status === 'running' && (
                    <button type="button" className="pn-btn" onClick={() => onEnabled(p.id, false)}>Disable</button>
                  )}
                  {p.status === 'needs-key' && (
                    <button type="button" className="pn-btn on" onClick={() => onKey(p.id)}>Add key</button>
                  )}
                  {p.status === 'off' && (
                    <button type="button" className="pn-btn" onClick={() => onEnabled(p.id, true)}>Enable</button>
                  )}
                  {p.status === 'needs-key' && (
                    <button type="button" className="pn-btn" onClick={() => onEnabled(p.id, false)}>Disable</button>
                  )}
                  <button type="button" className="pn-btn" onClick={() => onRemove(p.id)}>Remove</button>
                </div>
              </div>
              {p.command && <code className="pn-command">{p.command}</code>}
              {p.status === 'needs-key' && (
                <p className="pn-note">The key stays on {host}. Adding it here only records that the machine has one.</p>
              )}
            </div>
          )
        })}
      </div>

      <h2 className="pn-section">Marketplace</h2>
      <label className="pn-search">
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search the marketplace"
          spellCheck={false}
          aria-label="Search the marketplace"
        />
      </label>
      {catalog.length === 0 && <p className="pn-empty">Nothing in the marketplace matches that.</p>}
      <div className="pn-list pn-list--wide">
        {catalog.map((entry) => {
          const hosts = world.plugins.filter((p) => p.catalogId === entry.id).map((p) => deviceName(world.devices, p.deviceId))
          return (
            <div key={entry.id} className="pn-row pn-row--stack">
              <div className="pn-row-main">
                <div>
                  <div className="pn-name">{entry.name}</div>
                  <div className="pn-detail">
                    {entry.detail}
                    {entry.needsKey ? ' · Needs a key' : ''}
                    {hosts.length > 0 ? ` · On ${hosts.join(', ')}` : ''}
                  </div>
                </div>
                <button type="button" className="pn-btn" onClick={() => setPicking(picking === entry.id ? null : entry.id)}>
                  {picking === entry.id ? 'Cancel' : 'Install'}
                </button>
              </div>
              <code className="pn-command">{entry.command}</code>
              {picking === entry.id && (
                <div className="pn-pick">
                  {world.devices.map((d) => {
                    const Icon = deviceIcon(d.kind)
                    const ready = d.status === 'online'
                    const installed = world.plugins.some((p) => p.catalogId === entry.id && p.deviceId === d.id)
                    return (
                      <button
                        key={d.id}
                        type="button"
                        disabled={!ready || installed}
                        onClick={() => {
                          onInstall(entry.id, d.id)
                          setPicking(null)
                        }}
                      >
                        <span className="pn-pick-name">
                          <Icon size={14} weight="light" />
                          {d.name}
                        </span>
                        <span>{installed ? 'Installed' : ready ? 'Install here' : d.status === 'waiting' ? 'Has not checked in' : 'Offline'}</span>
                      </button>
                    )
                  })}
                </div>
              )}
            </div>
          )
        })}
      </div>
      {adding && (
        <AddPluginModal
          devices={world.devices}
          plugins={world.plugins}
          onClose={() => setAdding(false)}
          onAdd={(input) => {
            onAdd(input)
            setAdding(false)
          }}
        />
      )}
    </div>
  )
}

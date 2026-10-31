// Catalog plugin card in the 2-column Discover grid. Shows the icon tile,
// name, 2-line clamped description, metadata with non-sandboxed warning,
// and an Install / Update / Installed / Unavailable action.

import PluginIcon from './PluginIcon.jsx'
import { entryState, notSandboxed, pluginTitle } from './plugins.js'

export function PluginCard({ entry, onInstall, onInspect }) {
  if (!entry) return null

  const displayName = pluginTitle(entry.display_name ?? entry.name ?? '')
  const state = entryState(entry)
  const isNotSandboxed = Boolean(entry.not_sandboxed) || notSandboxed(entry)
  const versionOrCategory = entry.version || entry.category
  const metaParts = [entry.author, entry.marketplace, versionOrCategory].filter(Boolean)

  const handleInspect = () => {
    onInspect?.(entry)
  }

  const handleKeyDown = (e) => {
    if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) {
      e.preventDefault()
      onInspect?.(entry)
    }
  }

  return (
    <div
      role="button"
      tabIndex={0}
      className="pg-card"
      onClick={handleInspect}
      onKeyDown={handleKeyDown}
    >
      <PluginIcon
        name={displayName}
        id={entry.id}
        category={entry.category}
        homepage={entry.homepage}
        size={36}
      />
      <div className="pg-card-body">
        <div className="pg-card-name">{displayName}</div>
        {entry.description && (
          <p className="pg-card-desc">{entry.description}</p>
        )}
        <div className="pg-card-meta">
          {metaParts.length > 0 && (
            <span className="pg-card-meta-text">{metaParts.join(' · ')}</span>
          )}
          {isNotSandboxed && (
            <span className="pg-chip pg-warn" title="Runs as you, not sandboxed">
              Not sandboxed
            </span>
          )}
        </div>
      </div>
      <div className="pg-card-action">
        {state === 'available' && (
          <button
            type="button"
            className="pn-btn pg-card-btn pg-card-btn--install"
            onClick={(e) => {
              e.stopPropagation()
              onInstall?.(entry)
            }}
          >
            Install
          </button>
        )}
        {state === 'update' && (
          <button
            type="button"
            className="pn-btn pg-card-btn pg-card-btn--update"
            onClick={(e) => {
              e.stopPropagation()
              onInstall?.(entry)
            }}
          >
            Update
          </button>
        )}
        {state === 'installed' && (
          <span className="pg-card-status pg-card-status--installed">Installed</span>
        )}
        {state === 'unavailable' && (
          <span className="pg-card-status">Can&apos;t install</span>
        )}
      </div>
    </div>
  )
}

export default PluginCard

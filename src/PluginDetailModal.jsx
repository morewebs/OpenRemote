// Dedicated modal showing manifest details, runtime facts, requested powers,
// and install/update actions for a catalog or installed plugin.

import { useRef } from 'react'
import { ArrowSquareOut, Warning, X } from '@phosphor-icons/react'
import PluginIcon from './PluginIcon.jsx'
import {
  CLAUDE_PART_LABEL,
  POWER_LABEL,
  POWER_SHORT,
  RUNTIME_LABEL,
  pluginTitle,
  entryState,
  notSandboxed,
} from './plugins.js'
import { openExternal, webLink } from './platform.js'
import { useDialog } from './dialog-focus.js'
import './devices.css'
import './plugins.css'

export function PluginDetailModal({ entry, onClose, onInstall }) {
  const panelRef = useRef(null)
  const handleClose = () => {
    onClose?.()
  }

  const handleInstall = () => {
    onInstall?.(entry)
    onClose?.()
  }

  // A detail sheet with no entry renders nothing, and holds nothing: an Esc
  // there belongs to whatever is underneath. When it does open, the family's
  // shared plumbing takes over (focus in, Tab kept inside, the app inert
  // behind, focus back on the opener).
  useDialog(panelRef, handleClose, { open: Boolean(entry) })

  if (!entry) return null

  const displayName = pluginTitle(entry.display_name ?? entry.name ?? '')
  const technicalId = entry.id ?? entry.name ?? ''
  const state = entryState(entry)
  const isNotSandboxed = Boolean(entry.not_sandboxed) || notSandboxed(entry)
  const fromSource = entry.from || entry.marketplace || entry.source_kind
  // The catalog says where its homepage is; the button only exists when
  // that is a web page, since anything else would be handed to the
  // system's handler for that scheme.
  const homepage = webLink(entry.homepage)

  return (
    <div className="dv-modal-backdrop" onClick={handleClose}>
      <div
        className="dv-modal pg-detail-modal"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={displayName}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="dv-modal-head pg-detail-modal-head">
          <div className="pg-detail-modal-header-main">
            <PluginIcon
              name={displayName}
              id={technicalId}
              category={entry.category}
              homepage={entry.homepage}
              size={40}
            />
            <div className="pg-detail-modal-header-info">
              <div className="pg-detail-modal-title-row">
                <h2 className="dv-modal-title pg-details-name">{displayName}</h2>
                {entry.version && (
                  <span className="pg-version pg-version-badge">{entry.version}</span>
                )}
              </div>
              <div className="pg-detail-modal-meta">
                {technicalId && (
                  <span className="pg-details-id pv">{technicalId}</span>
                )}
                {entry.author && (
                  <span className="pg-detail-modal-author">by {entry.author}</span>
                )}
              </div>
            </div>
          </div>
          <button
            type="button"
            className="dv-modal-close"
            onClick={handleClose}
            title="Close"
            aria-label="Close"
          >
            <X size={14} weight="bold" />
          </button>
        </div>

        <div className="pg-detail-modal-body">
          {entry.description && (
            <p className="pg-details-desc">{entry.description}</p>
          )}

          <div className="pg-review">
            {entry.version && (
              <div className="pg-review-row">
                <span className="pg-review-label">Version</span>
                <span>{entry.version}</span>
              </div>
            )}
            {entry.author && (
              <div className="pg-review-row">
                <span className="pg-review-label">By</span>
                <span>{entry.author}</span>
              </div>
            )}
            {fromSource && (
              <div className="pg-review-row">
                <span className="pg-review-label">From</span>
                <span className="pg-wrap pv">{fromSource}</span>
              </div>
            )}
            {entry.runtime && (
              <div className="pg-review-row">
                <span className="pg-review-label">Runs as</span>
                <span>{RUNTIME_LABEL[entry.runtime] ?? entry.runtime}</span>
              </div>
            )}
            {entry.claude_parts?.length > 0 && (
              <div className="pg-review-row">
                <span className="pg-review-label">Carries</span>
                <span>
                  {entry.claude_parts
                    .map((p) => CLAUDE_PART_LABEL[p] ?? p)
                    .join(', ')}
                </span>
              </div>
            )}
            <div className="pg-review-row">
              <span className="pg-review-label">It may</span>
              {entry.powers == null ? (
                <span className="pg-muted">Shown when you review it</span>
              ) : entry.powers.length ? (
                <ul className="pg-powers">
                  {entry.powers.map((w) => (
                    <li key={w}>{POWER_LABEL[w] ?? w}</li>
                  ))}
                </ul>
              ) : (
                <span>Nothing beyond itself</span>
              )}
            </div>
          </div>

          {isNotSandboxed && (
            <div className="pg-facts">
              <span className="pg-chip pg-warn">
                <Warning size={13} weight="bold" />
                Runs as you, not sandboxed
              </span>
              {(entry.powers ?? [])
                .filter((w) => w !== 'run_programs')
                .map((w) => (
                  <span key={w} className="pg-chip">
                    {POWER_SHORT[w] ?? w}
                  </span>
                ))}
            </div>
          )}

          {homepage && (
            <div className="pg-detail-modal-homepage">
              <button
                type="button"
                className="pn-btn pg-homepage-btn"
                title={homepage}
                onClick={() => openExternal(homepage).catch(() => {})}
              >
                <ArrowSquareOut size={14} />
                <span>Homepage</span>
              </button>
            </div>
          )}

          {entry.why_not && (
            <p className="pg-sub pg-why-not">
              OpenRemote can&apos;t install it: {entry.why_not}.
            </p>
          )}
        </div>

        <div className="pg-review-actions pg-detail-modal-foot">
          <button type="button" className="pn-btn" onClick={handleClose}>
            Close
          </button>
          {state === 'available' && (
            <button type="button" className="dv-act" onClick={handleInstall}>
              Install
            </button>
          )}
          {state === 'update' && (
            <button type="button" className="dv-act" onClick={handleInstall}>
              Update
            </button>
          )}
          {state === 'installed' && (
            <button type="button" className="pn-btn" disabled>
              Installed
            </button>
          )}
          {state === 'unavailable' && (
            <button type="button" className="pn-btn" disabled>
              Can&apos;t install
            </button>
          )}
        </div>
      </div>
    </div>
  )
}

export default PluginDetailModal

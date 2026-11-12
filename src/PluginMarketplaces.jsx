// Plugins > Marketplaces: where Discover's plugins come from. A
// marketplace is a Claude Code catalog - or one in a similar layout, which
// the daemon reads the same way - added from owner/repo, a git URL, a
// folder or a link to a marketplace.json. Off hides it from Discover;
// removing it keeps what was installed from it. OpenRemote's own can be
// turned off, not removed.

import { useState } from 'react'
import { useConsole } from './state/console.jsx'
import { canPickFolder, pickFolder } from './pick-folder.js'
import { fetchedLabel } from './plugins.js'
import { errorCopy } from './error-copy.js'

// Where a marketplace is fetched from, in a line.
function sourceText(source) {
  if (!source) return ''
  if (source.kind === 'github') return `github.com/${source.repo}${source.ref ? `#${source.ref}` : ''}`
  if (source.kind === 'git') return `${source.url}${source.ref ? `#${source.ref}` : ''}`
  if (source.kind === 'directory') return source.path
  if (source.kind === 'url') return source.url
  return ''
}

export default function PluginMarketplaces() {
  const { marketplaces, addMarketplace, refreshMarketplace, setMarketplaceEnabled, removeMarketplace } =
    useConsole()
  const [value, setValue] = useState('')
  const [adding, setAdding] = useState(false)
  const [busy, setBusy] = useState(null)
  const [confirmRemove, setConfirmRemove] = useState(null)
  const [error, setError] = useState(null)

  const add = async () => {
    const source = value.trim()
    if (!source) return
    setAdding(true)
    setError(null)
    try {
      await addMarketplace(source)
      setValue('')
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setAdding(false)
    }
  }

  const run = async (name, action) => {
    setBusy(name)
    setError(null)
    try {
      await action()
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setBusy(null)
    }
  }

  const browse = async () => {
    const path = await pickFolder('Choose the marketplace folder')
    if (path) setValue(path)
  }

  return (
    <div className="pg-marketplaces-view">
      <h2 className="pn-section">Add a marketplace</h2>
      <p className="pn-lead">
        Add a Claude Code plugin marketplace: a GitHub owner/repo, a git URL, a folder, or a link.
        One built in a similar layout works too. Off hides its plugins from Discover.
      </p>
      <div className="pg-add-row">
        <input
          className="dv-command pg-add-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add()
          }}
          placeholder="owner/repo"
          spellCheck={false}
          autoComplete="off"
          aria-label="Where the marketplace is"
        />
        {canPickFolder && (
          <button type="button" className="pn-btn" onClick={browse}>
            Browse
          </button>
        )}
        <button type="button" className="dv-act" onClick={add} disabled={!value.trim() || adding}>
          {adding ? 'Fetching…' : 'Add'}
        </button>
      </div>
      {error && <p className="pn-error">{error}</p>}

      <h2 className="pn-section">Added</h2>
      {(marketplaces ?? []).length === 0 && (
        <p className="pn-empty">
          No marketplace has been added yet. Everything under Plugins works without one: a marketplace
          only adds Discover's catalog and its updates.
        </p>
      )}
      <div className="pn-list pg-list-full">
        {(marketplaces ?? []).map((m) => (
          <div className="pn-row pn-row--stack pg-mkt-card" key={m.name}>
            <div className="pn-row-main">
              <div>
                <div className="pn-name">{m.name}</div>
                <div className="pn-detail">
                  {m.description ?? (m.owner ? `By ${m.owner}` : 'A plugin marketplace')}
                </div>
              </div>
              <div className="pn-actions">
                <button
                  type="button"
                  className="pn-btn"
                  disabled={busy === m.name}
                  onClick={() => run(m.name, () => refreshMarketplace(m.name))}
                >
                  {busy === m.name ? 'Fetching…' : 'Refresh'}
                </button>
                <button
                  type="button"
                  className={m.enabled ? 'pn-btn on' : 'pn-btn'}
                  onClick={() => run(m.name, () => setMarketplaceEnabled(m.name, !m.enabled))}
                >
                  {m.enabled ? 'On' : 'Off'}
                </button>
                {m.removable &&
                  (confirmRemove === m.name ? (
                    <button
                      type="button"
                      className="pn-btn pn-btn--danger"
                      onClick={() => {
                        setConfirmRemove(null)
                        run(m.name, () => removeMarketplace(m.name))
                      }}
                    >
                      Remove it
                    </button>
                  ) : (
                    <button type="button" className="pn-btn" onClick={() => setConfirmRemove(m.name)}>
                      Remove
                    </button>
                  ))}
              </div>
            </div>
            <div className="pg-source pv">{sourceText(m.source)}</div>
            <div className="pg-sub pg-mkt-meta">
              {m.refreshed_at && !m.error ? (
                <span className="pg-chip">{m.plugin_count} {m.plugin_count === 1 ? 'plugin' : 'plugins'}</span>
              ) : null}
              <span className="pg-mkt-status">{fetchedLabel(m)}</span>
            </div>
            {m.error && <div className="pg-sub pg-failed">Couldn&apos;t fetch it: {m.error}</div>}
          </div>
        ))}
      </div>
      <p className="pn-lead pg-mcp-note">
        Removing a marketplace keeps what you installed from it; those plugins just stop getting updates.
      </p>
    </div>
  )
}

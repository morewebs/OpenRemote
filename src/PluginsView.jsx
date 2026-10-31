// Plugins, in three tabs. Discover: every plugin the marketplaces offer.
// Installed: the six harnesses OpenRemote drives, built in, and the
// plugins the user installed - each with its runtime and the powers it
// was given. Marketplaces: where Discover's plugins come from. MCP servers
// set up by hand live in Settings, under MCP servers.

import { useState } from 'react'
import { MagnifyingGlass, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { HarnessMark } from './brand-marks.jsx'
import InstallPluginModal from './InstallPluginModal.jsx'
import PluginDiscover from './PluginDiscover.jsx'
import PluginIcon from './PluginIcon.jsx'
import PluginMarketplaces from './PluginMarketplaces.jsx'
import {
  INSTALLED_EMPTY_NONE,
  POWER_SHORT,
  RUNTIME_LABEL,
  clashes,
  installedCountLine,
  installedEmptyLine,
  installedSearchHint,
  notSandboxed,
  pluginTitle,
  searchInstalled,
  sourceLabel,
} from './plugins.js'
import { useArmedRow } from './use-armed-row.js'
import { errorCopy } from './error-copy.js'
import './devices.css'
import './panels.css'
import './plugins.css'

const TABS = [
  { id: 'discover', label: 'Discover' },
  { id: 'installed', label: 'Installed' },
  { id: 'marketplaces', label: 'Marketplaces' },
]

export default function PluginsView() {
  const [tab, setTab] = useState('discover')
  // What the Install dialog is open for: a Discover entry, or a direct
  // install (folder, git repo, archive).
  const [installing, setInstalling] = useState(null)
  const { pluginsError, marketplacesError, refreshPlugins, refreshMarketplaces } = useConsole()

  // One field carries both refreshes (the state layer's), so the row must not
  // name one of them: a failure on the marketplace side used to read as a
  // plugin-list failure whose Retry then cleared it. The row names both
  // surfaces and retries both, marketplaces last. Each refresher keeps its own
  // failure in the shared field rather than throwing, so the last one to run
  // is the one whose answer the row shows: a marketplace that is still failing
  // puts the row straight back instead of being declared fine.
  // One row carries whichever refresh failed, because a person reading it
  // wants the truth about the list below rather than a diagnosis of which
  // request went wrong. Its retry refreshes the surface that failed, and both
  // surfaces when both failed, so a marketplace that is still failing is not
  // cleared by a plugin refresh that never touched it.
  const retryFailed = async () => {
    if (pluginsError) await refreshPlugins()
    if (marketplacesError) await refreshMarketplaces()
  }
  const refreshCopy =
    pluginsError && marketplacesError
      ? 'Could not refresh the plugins or the marketplaces, so what is below may be out of date.'
      : pluginsError
        ? 'Could not refresh the plugins, so the installed list may be out of date.'
        : 'Could not refresh the marketplaces, so Discover may be missing plugins.'

  return (
    <div className="panel pg-panel">
      <header className="dv-head pn-head pg-head">
        <div>
          <h1 className="dv-title">Plugins</h1>
          <p className="dv-meta">
            The harnesses OpenRemote drives, and what you add to it. Each one is on this computer.
          </p>
        </div>
        {/* The dialog's own title, so the door and the panel agree: the dialog
            behind this button takes a folder, a git repo or an archive, and
            naming all three here would list what its tabs already say. */}
        <button type="button" className="dv-act" onClick={() => setInstalling({ entry: null })}>
          Install a plugin
        </button>
      </header>

      <div className="pg-tabs" data-tab={tab} role="tablist" aria-label="Plugins">
        <span className="pg-tabs-thumb" aria-hidden="true" />
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            className={tab === t.id ? 'pg-tab on' : 'pg-tab'}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {(pluginsError || marketplacesError) && (
        <p className="pn-error pg-refresh-error" role="alert">
          {refreshCopy}
          <button type="button" className="pn-btn" onClick={retryFailed}>
            Retry
          </button>
        </p>
      )}

      {/* Discover stays mounted across the tabs, hidden rather than dropped:
          its search, its two filters and its expanded sections are view
          state the user typed, and going to Installed to check something
          and coming back used to empty all of it. */}
      <div className="pg-stage" hidden={tab !== 'discover'}>
        <PluginDiscover onInstall={(entry) => setInstalling({ entry })} />
      </div>
      {tab === 'installed' && (
        <div className="pg-stage">
          <InstalledPlugins onUpdate={(entry) => setInstalling({ entry })} />
        </div>
      )}
      {tab === 'marketplaces' && (
        <div className="pg-stage">
          <PluginMarketplaces />
        </div>
      )}

      {installing && <InstallPluginModal entry={installing.entry} onClose={() => setInstalling(null)} />}
    </div>
  )
}

function InstalledPlugins({ onUpdate }) {
  const { plugins, catalog, marketplaces, setPluginEnabled, removePlugin } = useConsole()
  const [confirmRemove, setConfirmRemove] = useState(null)
  // The arm ends when the row scrolls out of view or loses focus, so a first
  // press cannot outlive the user's attention (use-armed-row.js).
  const armWatch = useArmedRow(confirmRemove, setConfirmRemove)
  // The row whose removal is in flight. A second press used to start a
  // second delete and a second refresh, so the list could settle one
  // action behind; the pane disables the button and says what it is doing
  // instead.
  const [removing, setRemoving] = useState(null)
  const [error, setError] = useState(null)
  // The installed list's search. It narrows what the list draws and never
  // reorders it; plugins.js owns what a word may match.
  const [query, setQuery] = useState('')
  const builtin = plugins?.builtin ?? []
  const installed = plugins?.installed ?? []
  const shown = searchInstalled(installed, query)
  const filtering = query.trim().length > 0
  const entries = new Map((catalog?.entries ?? []).map((e) => [e.id, e]))
  const known = new Set((marketplaces ?? []).map((m) => m.name))
  const clashing = clashes(installed)

  const run = async (action) => {
    setError(null)
    try {
      await action()
    } catch (err) {
      setError(errorCopy(err))
    }
  }

  return (
    <div className="pg-installed-view">
      {clashing.map((c) => (
        <p key={c.name} className="pg-warn-block pg-clash">
          Two plugins named {c.name} are on: {c.ids.join(' and ')}. Their skills and commands share
          the name, so a Claude chat may mix them up. Turn one off to be sure.
        </p>
      ))}

      <h2 className="pn-section">Built in</h2>
      <p className="pn-lead">Off hides a harness from every picker. Its chats keep working.</p>
      <div className="pn-list pg-list-full pn-list--tight">
        {builtin.map((p) => (
          <div className="pn-row" key={p.id}>
            <div className="pg-main">
              <div className="pg-icon-tile">
                <HarnessMark harness={p.id} size={18} />
              </div>
              <div>
                <div className="pn-name">{p.name}</div>
                <div className="pn-detail">
                  {p.available ? `Harness${p.version ? ` · ${p.version}` : ''}` : 'Harness · not installed'}
                </div>
              </div>
            </div>
            <button
              type="button"
              className={p.enabled ? 'pn-btn on' : 'pn-btn'}
              onClick={() => run(() => setPluginEnabled(p.id, !p.enabled))}
            >
              {p.enabled ? 'On' : 'Off'}
            </button>
          </div>
        ))}
      </div>

      <h2 className="pn-section">Installed</h2>
      {/* The list's one way in. A field over an empty list would be a control
          with nothing to act on, so it appears with the first installed
          plugin; its hint appears only once the list is long enough that a
          person scans it. Escape clears the query, then leaves the field, the
          way the Discover search and the sidebar search do. */}
      {installed.length > 0 && (
        <div className="pg-installed-toolbar">
          <div className="pg-search-box">
            <MagnifyingGlass size={15} className="pg-search-icon" />
            <input
              type="text"
              className="pg-search-input"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key !== 'Escape') return
                e.preventDefault()
                if (query) setQuery('')
                else e.currentTarget.blur()
              }}
              placeholder={installedSearchHint(installed.length)}
              spellCheck={false}
              autoComplete="off"
              aria-label="Search installed plugins"
            />
            {query && (
              <button
                type="button"
                className="pg-search-clear"
                onClick={() => setQuery('')}
                aria-label="Clear search"
                title="Clear search"
              >
                <X size={13} weight="bold" />
              </button>
            )}
          </div>
          {/* The count only while a query is on: at rest the section has no
              count of its own and a second one would be noise. */}
          {filtering && shown.length > 0 && (
            <p className="pg-installed-count">{installedCountLine(shown.length, installed.length)}</p>
          )}
        </div>
      )}
      {installed.length === 0 && <p className="pn-empty">{INSTALLED_EMPTY_NONE}</p>}
      {/* A query that found nothing is not an empty list: it names the query
          rather than inviting an install. */}
      {filtering && installed.length > 0 && shown.length === 0 && (
        <p className="pn-empty">{installedEmptyLine(query)}</p>
      )}
      <div className="pn-list pg-list-full">
        {shown.map((p) => {
          const entry = entries.get(p.id)
          const orphaned = p.marketplace !== 'local' && !known.has(p.marketplace)
          const author = p.author ?? entry?.author
          return (
            <div className="pn-row pn-row--stack" key={p.id}>
              <div className="pn-row-main">
                <div className="pg-main">
                  <PluginIcon
                    name={p.name}
                    id={p.id}
                    category={entry?.category}
                    homepage={entry?.homepage}
                    size={36}
                  />
                  <div>
                    <div className="pn-name">
                      {pluginTitle(p.name)}
                      {p.version && <span className="pg-version">{p.version}</span>}
                      {author && <span className="pg-author-badge">by {author}</span>}
                      <span className="pg-badge-marketplace">@{p.marketplace}</span>
                    </div>
                    <div className="pn-detail">{p.description ?? RUNTIME_LABEL[p.runtime]}</div>
                  </div>
                </div>
                <div className="pn-actions">
                  {entry?.update && (
                    <button type="button" className="pn-btn pg-update" onClick={() => onUpdate(entry)}>
                      Update{entry.version ? ` to ${entry.version}` : ''}
                    </button>
                  )}
                  <button
                    type="button"
                    className={p.enabled ? 'pn-btn on' : 'pn-btn'}
                    onClick={() => run(() => setPluginEnabled(p.id, !p.enabled))}
                  >
                    {p.enabled ? 'On' : 'Off'}
                  </button>
                  {confirmRemove === p.id ? (
                    <button
                      type="button"
                      className="pn-btn pn-btn--danger"
                      ref={armWatch.buttonRef}
                      onBlur={armWatch.onBlur}
                      disabled={removing === p.id}
                      onClick={() => {
                        setConfirmRemove(null)
                        setRemoving(p.id)
                        run(() => removePlugin(p.id)).finally(() => setRemoving(null))
                      }}
                    >
                      {removing === p.id ? 'Removing…' : 'Remove it'}
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="pn-btn"
                      disabled={removing != null}
                      onClick={() => setConfirmRemove(p.id)}
                    >
                      Remove
                    </button>
                  )}
                </div>
              </div>
              <div className="pg-facts">
                <span className="pg-chip">{RUNTIME_LABEL[p.runtime] ?? p.runtime}</span>
                {(p.powers ?? []).map((w) => (
                  <span key={w} className="pg-chip">
                    {POWER_SHORT[w] ?? w}
                  </span>
                ))}
                {notSandboxed(p) && <span className="pg-chip pg-warn">Runs as you, not sandboxed</span>}
              </div>
              <div className="pg-source pv">{sourceLabel(p.source)}</div>
              {orphaned && (
                <div className="pg-sub">Its marketplace was removed: it keeps working, with no more updates.</div>
              )}
              {confirmRemove === p.id && (
                // What the second press takes, in the row it takes it from: a
                // tooltip is not saying it before it happens, because a person
                // reading the armed row is not hovering anything.
                <div className="pg-sub">
                  Its folder on this computer is deleted. Its marketplace still lists it, so it can be
                  installed again from there.
                </div>
              )}
            </div>
          )
        })}
      </div>
      <p className="pn-lead pg-mcp-note">MCP servers you set up by hand are in Settings, under MCP servers.</p>
      {error && <p className="pn-error">{error}</p>}
    </div>
  )
}

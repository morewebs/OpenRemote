// The new tab page's shortcuts and dev-server row (ported from the browser
// branch's BrowserNewTab, 0d3211a, dfcf317): up to eight shortcuts under the
// search box - the user's own first, then the sites read most, as the
// daemon ranks them (GET /browser/tiles) - and one quiet row of the dev
// servers seen in chats lately (devservers.js). Drawn only for a tab that is
// not private: a private tab's page lists nothing this computer has read.

import { useEffect, useRef, useState } from 'react'
import { DotsThreeVertical, PencilSimple, Plus, Terminal, Trash, X } from '@phosphor-icons/react'
import ContextMenu from './ContextMenu.jsx'
import { addressFor } from './browser-omnibox.js'
import { canAddTile, serverRows, tileLabel, tileLetter, tilesFrom } from './browser-newtab.js'
import { recentDevServers, subscribeDevServers } from './devservers.js'
import { browserCall } from './browser-store.js'
import { errorCopy } from './error-copy.js'
import { useDialog } from './dialog-focus.js'
import { useBack } from './use-back.js'
import { usePrivacyMode } from './privacy.js'
import './devices.css'
import './panels.css'
import './browser-settings.css'

// Adding or editing a shortcut: its name and address. A refusal stays in
// the dialog, in errorCopy's words, so nothing typed is lost.
function TileDialog({ tile, onSave, onClose }) {
  const panelRef = useRef(null)
  const [title, setTitle] = useState(tile?.title ?? '')
  const [url, setUrl] = useState(tile?.url ?? '')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)
  useDialog(panelRef, onClose)
  useBack(true, onClose)

  const save = async (e) => {
    e.preventDefault()
    const address = addressFor(url)
    if (!address) {
      setError('Type an address, like example.com.')
      return
    }
    setBusy(true)
    try {
      await onSave({ title: title.trim(), url: address })
      onClose()
    } catch (err) {
      setError(errorCopy(err))
      setBusy(false)
    }
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <form
        ref={panelRef}
        tabIndex={-1}
        className="dv-modal dv-modal--form bw-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={tile ? 'Edit shortcut' : 'Add shortcut'}
        onSubmit={save}
      >
        <div className="dv-modal-head">
          <span className="dv-modal-title">{tile ? 'Edit shortcut' : 'Add shortcut'}</span>
          <button type="button" className="dv-modal-close" aria-label="Close" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        <label className="bw-field">
          <span>Name</span>
          <input className="bw-field-input" value={title} onChange={(e) => setTitle(e.target.value)} spellCheck={false} autoFocus />
        </label>
        <label className="bw-field">
          <span>URL</span>
          <input
            className="bw-field-input"
            value={url}
            onChange={(e) => {
              setUrl(e.target.value)
              setError(null)
            }}
            spellCheck={false}
            placeholder="example.com"
          />
        </label>
        {error && <p className="bw-dialog-error">{error}</p>}
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" className="pn-btn bw-primary" disabled={busy || !url.trim()}>
            Done
          </button>
        </div>
      </form>
    </div>
  )
}

export default function BrowserShortcuts({ onOpen, onError }) {
  const [tiles, setTiles] = useState([])
  const [menu, setMenu] = useState(null)
  const [dialog, setDialog] = useState(null)
  // recentDevServers() builds a new list on every read, so it is kept in
  // state and read again on each change (a snapshot store would loop).
  const [servers, setServers] = useState(() => recentDevServers())
  useEffect(() => subscribeDevServers(() => setServers(recentDevServers())), [])
  const privacy = usePrivacyMode()
  const alive = useRef(true)
  useEffect(() => () => {
    alive.current = false
  }, [])

  // A daemon that does not answer leaves the page with its search box and
  // Recent list: the shortcuts are a glance, not something to warn about.
  const load = async () => {
    try {
      const got = await browserCall('GET', '/browser/tiles')
      if (alive.current) setTiles(tilesFrom(got))
    } catch {
      if (alive.current) setTiles([])
    }
  }
  useEffect(() => {
    load()
  }, [])

  const saveTile = async (next) => {
    const before = dialog?.tile
    if (before && before.url !== next.url) await browserCall('POST', '/browser/tiles/remove', { url: before.url })
    await browserCall('POST', '/browser/tiles', next)
    await load()
  }

  const removeTile = async (tile) => {
    try {
      await browserCall('POST', '/browser/tiles/remove', { url: tile.url })
    } catch (err) {
      onError?.(errorCopy(err))
    }
    await load()
  }

  const rows = serverRows(servers)

  return (
    <>
      <div className="bw-tiles" aria-label="Shortcuts">
        {tiles.map((t) => (
          <div className="bw-tile" key={t.url}>
            <button type="button" className="bw-tile-main" title={privacy ? undefined : t.url} onClick={() => onOpen(t.url)}>
              <span className="bw-tile-icon">
                <span className="bw-tile-letter">{tileLetter(t)}</span>
              </span>
              <span className="bw-tile-title pv">{tileLabel(t)}</span>
            </button>
            <button
              type="button"
              className="bw-tile-more"
              title="Edit or remove"
              aria-label="Edit or remove"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect()
                setMenu({ tile: t, x: r.left, y: r.bottom + 4 })
              }}
            >
              <DotsThreeVertical size={14} weight="bold" />
            </button>
          </div>
        ))}
        {canAddTile(tiles) && (
          <div className="bw-tile">
            <button type="button" className="bw-tile-main" onClick={() => setDialog({ tile: null })}>
              <span className="bw-tile-icon">
                <Plus size={18} />
              </span>
              <span className="bw-tile-title">Add shortcut</span>
            </button>
          </div>
        )}
      </div>
      {rows.length > 0 && (
        <div className="bw-ntp-servers" aria-label="Dev servers in your chats">
          <span className="bw-ntp-label">Dev servers in your chats</span>
          {rows.map((s) => (
            <button key={s.url} type="button" className="bw-chip" title={s.url} onClick={() => onOpen(s.url)}>
              <Terminal size={12} />
              {s.label}
            </button>
          ))}
        </div>
      )}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            { label: 'Edit shortcut', icon: <PencilSimple size={14} />, onSelect: () => setDialog({ tile: menu.tile }) },
            { label: 'Remove', icon: <Trash size={14} />, danger: true, onSelect: () => removeTile(menu.tile) },
          ]}
        />
      )}
      {dialog && <TileDialog tile={dialog.tile} onSave={saveTile} onClose={() => setDialog(null)} />}
    </>
  )
}

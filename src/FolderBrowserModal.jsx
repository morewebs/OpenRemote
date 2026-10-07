// Choosing a folder on another machine (or here, when there is no native
// dialog): browse its folders and use one. A path is never typed - the
// same rule as the native dialog on this computer.

import { useEffect, useState } from 'react'
import { ArrowUp, Folder, House, X } from '@phosphor-icons/react'
import './devices.css'
import './folders.css'

export default function FolderBrowserModal({ api, where, start, onChoose, onClose }) {
  const [listing, setListing] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(false)

  const open = async (path) => {
    setLoading(true)
    setError(null)
    try {
      setListing(await api.fsDirs(path))
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    open(start ?? null)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal fb-modal" role="dialog" aria-modal="true" aria-label="Choose a folder">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Choose a folder</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">The folder on {where} the harness will work in.</p>
        <div className="fb-path" title={listing?.path}>
          {listing?.path ?? ' '}
        </div>
        <div className="fb-list" aria-busy={loading}>
          {listing?.parent && (
            <button type="button" className="fb-row" onClick={() => open(listing.parent)}>
              <ArrowUp size={13} />
              Up
            </button>
          )}
          {listing && listing.home !== listing.path && (
            <button type="button" className="fb-row" onClick={() => open(listing.home)}>
              <House size={13} />
              Home
            </button>
          )}
          {(listing?.dirs ?? []).map((dir) => (
            <button type="button" key={dir.path} className="fb-row" onClick={() => open(dir.path)}>
              <Folder size={13} />
              {dir.name}
            </button>
          ))}
          {listing && listing.dirs.length === 0 && <p className="fb-empty">No folders inside.</p>}
          {listing?.truncated && <p className="fb-empty">Showing the first 2000 folders.</p>}
        </div>
        {error && <p className="dv-error">{error}</p>}
        <div className="fb-foot">
          <button type="button" className="dv-act" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className="dv-act primary"
            disabled={!listing || loading}
            onClick={() => onChoose(listing.path)}
          >
            Use this folder
          </button>
        </div>
      </div>
    </div>
  )
}

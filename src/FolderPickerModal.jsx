// The one folder picker: choose a folder on this computer or another of
// the user's machines, browsing its folders inside the app. A path is
// never typed - the same rule everywhere, local and remote alike.

import { useEffect, useRef, useState } from 'react'
import { ArrowUp, Folder, House, X } from '@phosphor-icons/react'
import './devices.css'
import './folders.css'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { usePrivacyMode } from './privacy.js'
import { errorCopy } from './error-copy.js'

export default function FolderPickerModal({ api, where, start, onChoose, onClose }) {
  const privacy = usePrivacyMode()
  useBack(true, onClose)
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  const [listing, setListing] = useState(null)
  const [error, setError] = useState(null)
  const [loading, setLoading] = useState(false)
  // Every open carries a token and its own stop. Over the mesh a slow
  // folder request can resolve after a later fast one (click a big
  // remote folder, then Home), and last-resolved-wins would clobber the
  // newer navigation - "Use this folder" then registers the wrong
  // workspace. A response whose token is stale is dropped, the older
  // request is stopped, and unmounting stops whatever is still in
  // flight.
  const openSeq = useRef(0)
  const inFlight = useRef(null)

  const open = async (path) => {
    openSeq.current += 1
    const token = openSeq.current
    inFlight.current?.abort()
    const controller = new AbortController()
    inFlight.current = controller
    setLoading(true)
    setError(null)
    try {
      const view = await api.fsDirs(path, { signal: controller.signal })
      if (token !== openSeq.current) return // a newer click owns the modal now
      setListing(view)
    } catch (err) {
      // Stopped on purpose (superseded or unmounted), not a failure.
      if (controller.signal.aborted || token !== openSeq.current) return
      setError(errorCopy(err))
    } finally {
      if (token === openSeq.current) setLoading(false)
    }
  }

  useEffect(() => {
    open(start ?? null)
    return () => {
      openSeq.current += 1 // any straggler response is stale
      inFlight.current?.abort()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal fb-modal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label="Choose a folder">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Choose a folder</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">The folder on <span className="pv">{where}</span> the app will work in.</p>
        <div className="fb-path pv" title={privacy ? undefined : listing?.path}>
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
              <span className="pv">{dir.name}</span>
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

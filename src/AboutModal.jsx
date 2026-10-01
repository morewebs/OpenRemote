import { useEffect } from 'react'
import { X } from '@phosphor-icons/react'
import './about.css'

export default function AboutModal({ onClose, daemonVersion }) {
  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div
      className="ab-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="ab-modal" role="dialog" aria-modal="true" aria-label="About OpenRemote">
        <div className="ab-head">
          <h2 className="ab-wordmark">OpenRemote</h2>
          <button className="ab-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <div className="ab-version">Version 0.1.0 · slice 1</div>
        {daemonVersion && <div className="ab-version">Daemon {daemonVersion}</div>}
        <div className="ab-sep" />
        <div className="ab-foot">Moreweb · 2026</div>
      </div>
    </div>
  )
}

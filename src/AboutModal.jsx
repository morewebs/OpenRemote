import { useEffect, useRef, useState } from 'react'
import { Check, X } from '@phosphor-icons/react'
import './about.css'

export default function AboutModal({ onClose, version, latest, reduceMotion, onUpdated }) {
  // idle → checking → available → updating → done
  const [phase, setPhase] = useState('idle')
  const timer = useRef(null)

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  useEffect(
    () => () => {
      clearTimeout(timer.current)
      clearInterval(timer.current)
    },
    [],
  )

  const check = () => {
    if (reduceMotion) {
      setPhase('available')
      return
    }
    setPhase('checking')
    timer.current = setTimeout(() => setPhase('available'), 900)
  }

  // the bar fills via a CSS animation on wall-clock; a single timeout
  // flips the phase — no per-tick interval to starve in hidden tabs
  const update = () => {
    if (reduceMotion) {
      setPhase('done')
      onUpdated?.()
      return
    }
    setPhase('updating')
    timer.current = setTimeout(() => {
      setPhase('done')
      onUpdated?.()
    }, 1700)
  }

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
        <div className="ab-version">
          Version {version} · dev
        </div>

        <div className="ab-sep" />

        {phase === 'idle' && (
          <button className="ab-btn" onClick={check}>
            Check for updates
          </button>
        )}
        {phase === 'checking' && <div className="ab-status">Checking for updates…</div>}
        {phase === 'available' && (
          <div className="ab-update">
            <div className="ab-available">Version {latest} is available</div>
            <button className="ab-btn" onClick={update}>
              Update now
            </button>
          </div>
        )}
        {phase === 'updating' && (
          <div className="ab-update">
            <div className="ab-bar">
              <span />
            </div>
            <div className="ab-status">Updating…</div>
          </div>
        )}
        {phase === 'done' && (
          <div className="ab-update">
            <div className="ab-done">
              <Check size={13} weight="bold" />
              Updated to {latest}
            </div>
            <div className="ab-status">Restart the app to finish.</div>
          </div>
        )}

        <div className="ab-foot">Moreweb · 2026</div>
      </div>
    </div>
  )
}

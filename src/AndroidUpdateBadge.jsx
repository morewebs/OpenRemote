import { useEffect, useState } from 'react'
import { ArrowCircleUp, ArrowClockwise, CheckCircle, CircleNotch } from '@phosphor-icons/react'
import { checkAndroidUpdate } from './android-updates.js'
import { openExternal } from './platform.js'

export default function AndroidUpdateBadge({ version }) {
  const [attempt, setAttempt] = useState(0)
  const [state, setState] = useState({ status: 'checking' })
  useEffect(() => {
    let active = true
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort(), 15_000)
    setState({ status: 'checking' })
    checkAndroidUpdate(version, { signal: controller.signal })
      .then((update) => { if (active) setState({ status: update ? 'available' : 'current', update }) })
      .catch(() => { if (active) setState({ status: 'error' }) })
      .finally(() => clearTimeout(timeout))
    return () => {
      active = false
      clearTimeout(timeout)
      controller.abort()
    }
  }, [version, attempt])

  const retry = () => setAttempt((n) => n + 1)
  return (
    <span className="ab-update-status" role="status" aria-live="polite">
      {state.status === 'checking' ? (
        <span className="ab-update-badge" aria-busy="true">
          <CircleNotch className="ab-update-spinner" size={16} aria-hidden="true" /> Checking…
        </span>
      ) : state.status === 'current' ? (
        <span className="ab-update-badge">
          <CheckCircle size={16} weight="fill" aria-hidden="true" /> Up to date
        </span>
      ) : state.status === 'available' ? (
        <button
          type="button"
          className="ab-update-badge ab-update-badge--available"
          title={`Download OpenRemote ${state.update.version}. Android will ask you to install the APK.`}
          aria-label={`Update to OpenRemote ${state.update.version}`}
          onClick={() => openExternal(state.update.url).then((opened) => {
            if (!opened) setState({ status: 'error' })
          }).catch(() => setState({ status: 'error' }))}
        >
          <ArrowCircleUp size={16} weight="fill" aria-hidden="true" /> Update available
        </button>
      ) : (
        <button type="button" className="ab-update-badge" onClick={retry} title="Couldn't check for updates. Tap to retry.">
          <ArrowClockwise size={16} aria-hidden="true" /> Retry check
        </button>
      )}
    </span>
  )
}

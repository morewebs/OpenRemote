import { useEffect, useState } from 'react'
import { useConsole } from './state/console.jsx'
import { daemonStatus } from './daemon-status.js'
import { hasTauri, isMobile } from './platform.js'
import './daemon-status.css'

// How long the button says "Restarting..." after a press. A restart closes
// the old daemon's stdin and waits for it to stop the harnesses it started,
// which is seconds by design; a second press in that window only restarts a
// daemon that is already restarting.
const RETRY_BUSY_MS = 5000

// The note while the screen is only waiting out a boot: the daemon has not
// failed at anything, so the copy must not promise a restart button that the
// screen is deliberately not offering.
const BOOTING_NOTE =
  'It starts with the app and usually takes a moment. This screen leaves by itself when it answers.'

// The screen the app shows while it has no daemon: what happened, when the
// shell has a reason to give, and one button that starts it again. It is
// not a dead end: the console keeps asking, so the panel leaves by itself
// the moment a daemon answers. `onManualConnect` keeps the door open for
// someone who runs the daemon themselves instead of the app's sidecar, and
// only on a computer: the phone app has no daemon of its own to point at,
// so an address form there would ask for something a phone cannot produce.
export default function DaemonStatus({ onManualConnect = null }) {
  const { daemon, connection, retryDaemon } = useConsole()
  const status = daemonStatus(daemon)
  // The console reaches this screen two ways: the shell said something is
  // wrong, or the boot grace ran out with the daemon still starting. Only the
  // first is a failure to restart - on a boot that is merely slow the button
  // would throw a healthy start away, so it waits for the shell's own reason.
  const booting = connection?.grace === true && !daemon?.error
  const [retrying, setRetrying] = useState(false)

  // A press is the only acknowledgement a person gets: the panel keeps the
  // same words while the shell works, and without this the button would take
  // another press and start a second restart on top of the first.
  useEffect(() => {
    if (!retrying) return undefined
    const timer = setTimeout(() => setRetrying(false), RETRY_BUSY_MS)
    return () => clearTimeout(timer)
  }, [retrying])

  const retryNow = () => {
    if (retrying) return
    setRetrying(true)
    retryDaemon()
  }

  return (
    <div className="ds">
      <div className="ds-panel" role="status" aria-live="polite">
        <h1 className="ds-title">{status.title}</h1>
        {/* The shell's own sentence, which can carry the sidecar's absolute
            path - the person's home folder - so it blurs like the rest. */}
        {status.detail && <p className="ds-detail pv">{status.detail}</p>}
        <p className="ds-note">{booting ? BOOTING_NOTE : status.note}</p>
        {/* The button restarts the shell's sidecar. A browser tab has no
            shell and no daemon to restart, so the offer is left out there
            rather than pressed into nothing. */}
        {status.action === 'try-now' && !booting && hasTauri && (
          <button className="ds-cta" onClick={retryNow} disabled={retrying}>
            {retrying ? 'Restarting...' : 'Try now'}
          </button>
        )}
        {onManualConnect && !isMobile && (
          <button className="ds-link" onClick={onManualConnect}>
            Connect by address instead
          </button>
        )}
      </div>
    </div>
  )
}

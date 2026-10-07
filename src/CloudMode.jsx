// Cloud's door: the main area while the mode switch sits on Cloud and this
// computer isn't signed in. The subject is the hero - the cloud with
// machines checking in around it, in the app's own status palette, drawn
// in once - then the words and one action. Signing in happens in the
// system browser; the daemon opens it and the poll notices when it's done.

import { useState } from 'react'
import { ArrowSquareOut, Copy, House } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import './cloudmode.css'

// One machine checking in: a small card with its status dot - green for
// online, hollow for not-yet. The same vocabulary the machine cards use.
function MachineNode({ x, y, online, delay }) {
  return (
    <g className="cm-node" style={{ '--cm-delay': `${delay}ms` }}>
      <rect x={x} y={y} width={26} height={19} rx={5} />
      <circle className={online ? 'cm-ndot on' : 'cm-ndot'} cx={x + 8} cy={y + 9.5} r={2.4} />
      <line x1={x + 14} y1={y + 9.5} x2={x + 21} y2={y + 9.5} />
    </g>
  )
}

// What the screen says for each sign-in state.
function words(cloud) {
  switch (cloud?.state) {
    case 'signing_in':
      return {
        title: 'Finish in your browser',
        body: "moreweb's sign-in page is open in your browser. Come back here once it says you're signed in.",
      }
    case 'relink':
      return {
        title: 'Sign in again',
        body: 'Your sign-in on this computer ended. Sign in to reach your other devices again.',
      }
    case 'revoked':
      return {
        title: 'This computer was removed',
        body: "It was taken out of your Cloud, and the synced chats it kept for your other devices were deleted here. Its Local chats are untouched. Sign in to add it again.",
      }
    default:
      return {
        title: 'Sign in to use Cloud',
        body: 'Cloud connects this computer with your others - servers, home machines, other laptops. Chats sync between them end to end encrypted; moreweb never sees or keeps them.',
      }
  }
}

export default function CloudMode({ onBackToLocal }) {
  const { cloud, connection, signInCloud, cancelCloudSignIn } = useConsole()
  const [link, setLink] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const { title, body } = words(cloud)
  const signingIn = cloud?.state === 'signing_in'

  const start = async () => {
    setBusy(true)
    setError(null)
    try {
      const started = await signInCloud()
      // No browser could be opened from here: hand over the link instead.
      setLink(started?.opened ? null : started?.authorize_url ?? null)
    } catch (err) {
      setError(err.message ?? String(err))
    } finally {
      setBusy(false)
    }
  }

  const cancel = async () => {
    setLink(null)
    try {
      await cancelCloudSignIn()
    } catch {
      /* the poll settles it */
    }
  }

  return (
    <div className="cloudmode">
      <div className="cm-horizon" aria-hidden="true" />
      <div className="cm-panel">
        <svg
          className="cm-hero"
          viewBox="0 0 260 170"
          role="img"
          aria-label="A cloud with machines checking in around it"
        >
          {/* the connectors: each machine reaching the cloud */}
          <line className="cm-link" style={{ '--cm-delay': '300ms' }} x1="34" y1="46" x2="86" y2="72" />
          <line className="cm-link" style={{ '--cm-delay': '420ms' }} x1="224" y1="38" x2="176" y2="66" />
          <line className="cm-link" style={{ '--cm-delay': '540ms' }} x1="18" y1="132" x2="82" y2="102" />
          <line className="cm-link" style={{ '--cm-delay': '660ms' }} x1="238" y1="126" x2="180" y2="100" />

          {/* the cloud itself - lucide's own cloud path, drawn in */}
          <path
            className="cm-cloud"
            d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"
            transform="translate(68 34) scale(5.2)"
          />

          <MachineNode x={8} y={36} online delay={300} />
          <MachineNode x={212} y={28} online delay={420} />
          <MachineNode x={4} y={124} delay={540} />
          <MachineNode x={226} y={118} delay={660} />
        </svg>

        <h1 className="cm-title">{title}</h1>
        <p className="cm-body">{body}</p>
        {(error || cloud?.error) && !signingIn && (
          <p className="cm-error" role="alert">
            {error ?? cloud.error}
          </p>
        )}

        {link && (
          <div className="cm-linkbox">
            <span className="cm-linktext">{link}</span>
            <button
              type="button"
              className="cm-back cm-copy"
              onClick={() => navigator.clipboard?.writeText(link)}
              title="Copy the sign-in link"
            >
              <Copy size={13} />
              Copy link
            </button>
          </div>
        )}

        <div className="cm-actions">
          {signingIn ? (
            <>
              <button type="button" className="cm-back primary" onClick={start} disabled={busy}>
                <ArrowSquareOut size={13} />
                Open the page again
              </button>
              <button type="button" className="cm-back" onClick={cancel}>
                Cancel
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                className="cm-back primary"
                onClick={start}
                disabled={busy || connection.state !== 'connected'}
              >
                <ArrowSquareOut size={13} />
                {cloud?.state === 'revoked' || cloud?.state === 'relink' ? 'Sign in again' : 'Sign in with moreweb'}
              </button>
              <button type="button" className="cm-back" onClick={onBackToLocal}>
                <House size={13} />
                Back to Local
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  )
}

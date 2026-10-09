import { useEffect, useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import './signin.css'
import { useBack } from './use-back.js'

// The harness's own sign-in, relayed. The daemon runs the CLI's own
// login command on the machine; the CLI's words stream in here (the
// browser URL it opened, the code it wants pasted), and the human's
// answers ride back through the same relay. The words are the CLI's -
// never ours.
export default function SignInModal({ harnessId, onDone, deviceId = null }) {
  useBack(true, onDone)
  const { signIn, feedSignIn, stopSignIn } = useConsole()
  const [view, setView] = useState(null)
  const [error, setError] = useState(null)
  const [answer, setAnswer] = useState('')
  const [closing, setClosing] = useState(false)
  const [stopping, setStopping] = useState(false)
  const transcriptRef = useRef(null)

  // One relay per modal: it starts the login and polls the beats until
  // the CLI exits. The modal closing does not stop a login the human may
  // still be finishing in the browser - the daemon's run continues.
  useEffect(() => {
    let cancelled = false
    signIn(harnessId, { onBeat: (beat) => !cancelled && setView(beat), deviceId }).catch((err) => {
      if (!cancelled) setError(err?.message ?? String(err))
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [harnessId])

  // The transcript follows its own tail - the newest word is on screen.
  useEffect(() => {
    transcriptRef.current?.scrollTo({ top: transcriptRef.current.scrollHeight })
  }, [view?.lines?.length])

  const settled = view?.done ?? null
  const running = view ? !view.done : true

  const submit = async () => {
    if (!answer.trim()) return
    try {
      await feedSignIn(harnessId, answer.trim(), deviceId)
      setAnswer('')
    } catch (err) {
      setError(err?.message ?? String(err))
    }
  }

  const close = () => {
    setClosing(true)
    // Closing a running relay stops it - an abandoned browser flow
    // should not block the next attempt.
    if (running && !stopping) {
      setStopping(true)
      stopSignIn(harnessId, deviceId).catch(() => {})
    }
    onDone()
  }

  return (
    <div
      className="si-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && (settled || error)) close()
      }}
    >
      <div className="si-modal" role="dialog" aria-modal="true" aria-label={`Sign in to ${harnessId}`}>
        <div className="si-head">
          <h2 className="si-title">Sign in</h2>
          <button
            className="si-close"
            onClick={close}
            title={settled || error ? 'Close' : 'Stop this attempt and close'}
          >
            <X size={14} weight="bold" />
          </button>
        </div>

        {error && (
          <>
            <p className="si-hint si-hint--error">{error}</p>
            <button className="si-primary" onClick={close}>
              Close
            </button>
          </>
        )}

        {!error && (
          <>
            <p className="si-hint">
              {settled
                ? settled.ok
                  ? 'The harness answered with its own sign-in words.'
                  : `The login command finished on its own: ${settled.text}`
                : 'The harness’s own login command is running - its words appear below. Finish in the browser it opened.'}
            </p>
            <div className="si-transcript" ref={transcriptRef}>
              {(view?.lines ?? []).map((line, i) => (
                <div className="si-line" key={i}>{line}</div>
              ))}
              {running && <div className="si-line si-line--live">…</div>}
            </div>
            {running && (
              <div className="si-answer">
                <input
                  className="si-input"
                  value={answer}
                  onChange={(e) => setAnswer(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault()
                      submit()
                    }
                  }}
                  placeholder="Paste the code the harness asked for"
                  spellCheck={false}
                  autoFocus
                  disabled={closing}
                />
                <button className="si-send" onClick={submit} disabled={!answer.trim() || closing}>
                  Send
                </button>
              </div>
            )}
            {running && (
              <button
                className="si-stop"
                onClick={() => {
                  setStopping(true)
                  stopSignIn(harnessId, deviceId).catch(() => {})
                }}
                disabled={stopping}
              >
                {stopping ? 'Stopping…' : 'Stop this attempt'}
              </button>
            )}
            {settled && (
              <button className="si-primary" onClick={close} disabled={closing}>
                {settled.ok ? 'Done' : 'Close'}
              </button>
            )}
          </>
        )}
      </div>
    </div>
  )
}

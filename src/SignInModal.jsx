import { useEffect, useRef, useState } from 'react'
import { ArrowSquareOut, Copy, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import './signin.css'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { errorCopy } from './error-copy.js'
import { addressLabel, readSignInWords } from './signin-words.js'
import { harnessName } from './harness-names.js'

// The harness's own sign-in, relayed. The daemon runs the CLI's own
// login command on the machine; the CLI's words stream in here (the
// browser URL it opened, the code it wants pasted), and the human's
// answers ride back through the same relay. The words are the CLI's -
// never ours.
//
// A login prints a wall: a notice, an authorize address hundreds of characters
// long, sometimes an aside about headless machines. The sheet leads with the one
// thing a person acts on - open that address - and keeps every word the harness
// said behind a press, verbatim, for the times the reading and the words
// disagree.
export default function SignInModal({ harnessId, onDone, deviceId = null }) {
  useBack(true, onDone)
  const { signIn, feedSignIn, stopSignIn } = useConsole()
  const panelRef = useRef(null)
  const [view, setView] = useState(null)
  const [error, setError] = useState(null)
  const [answer, setAnswer] = useState('')
  const [closing, setClosing] = useState(false)
  const [stopping, setStopping] = useState(false)
  const [openError, setOpenError] = useState(null)
  const [copied, setCopied] = useState(null)
  // null means nobody has pressed yet: the words start open when there is no
  // address to act on (then the words are all there is) and folded when there
  // is one (then the button is the screen).
  const [showWords, setShowWords] = useState(null)
  const transcriptRef = useRef(null)

  // One relay per modal: it starts the login and polls the beats until
  // the CLI exits. The modal closing does not stop a login the human may
  // still be finishing in the browser - the daemon's run continues - but
  // it does stop the poll reading it: nothing should keep requesting a
  // view nobody renders (a cancelled modal's error is silence, not a
  // banner).
  useEffect(() => {
    let cancelled = false
    const relay = signIn(harnessId, { onBeat: (beat) => !cancelled && setView(beat), deviceId })
    relay.promise.catch((err) => {
      if (!cancelled && !err?.cancelled) setError(errorCopy(err))
    })
    return () => {
      cancelled = true
      relay.cancel()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [harnessId])

  // The transcript follows its own tail - the newest word is on screen.
  useEffect(() => {
    transcriptRef.current?.scrollTo({ top: transcriptRef.current.scrollHeight })
  }, [view?.lines?.length])

  const settled = view?.done ?? null
  const running = view ? !view.done : true
  const read = readSignInWords(view?.lines)
  const wordsOpen = showWords ?? !read.address

  // Opening the address in the person's own browser: the one the CLI already
  // tried to open, offered again for the case where it did not, and the system
  // browser because that is where their sign-in already lives (a passkey, a
  // session that outlived this window).
  const openAddress = async () => {
    setOpenError(null)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('browser_open_external', { url: read.address })
    } catch {
      setOpenError('The browser did not open. Copy the link and open it yourself.')
    }
  }

  const copyAddress = async () => {
    setCopied(null)
    try {
      await navigator.clipboard.writeText(read.address)
      setCopied('copied')
    } catch {
      // The clipboard is a permission, and a refusal is the person's to see.
      setCopied('refused')
    }
  }

  const submit = async () => {
    if (!answer.trim()) return
    try {
      await feedSignIn(harnessId, answer.trim(), deviceId)
      setAnswer('')
    } catch (err) {
      setError(errorCopy(err))
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

  // The sign-in sheet had no Esc at all before the dialog plumbing: it held
  // no layer and answered no key, so the only ways out were its own buttons.
  // Esc now closes it through the same guarded close the X uses.
  useDialog(panelRef, close)

  return (
    <div
      className="si-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && (settled || error)) close()
      }}
    >
      <div className="si-modal" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={`Sign in to ${harnessName(harnessId)}`}>
        <div className="si-head">
          <h2 className="si-title">Sign in to {harnessName(harnessId)}</h2>
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
                  ? 'The app answered with its own sign-in words.'
                  : `The login command finished on its own: ${settled.text}`
                : read.address
                  ? 'Finish this in your browser. The app opened a page; this button opens it again.'
                  : 'The app’s own login command is running - its words appear below.'}
            </p>

            {read.address && (
              <div className="si-open">
                <button className="si-open-button" onClick={openAddress}>
                  <ArrowSquareOut size={15} weight="bold" />
                  Open the sign-in page
                </button>
                <div className="si-open-line">
                  <span className="si-open-host" title={read.address}>
                    {addressLabel(read.address)}
                  </span>
                  <button className="si-copy" onClick={copyAddress}>
                    <Copy size={12} />
                    {copied === 'copied' ? 'Copied' : copied === 'refused' ? 'Copy refused' : 'Copy link'}
                  </button>
                </div>
                {openError && <p className="si-hint si-hint--error">{openError}</p>}
              </div>
            )}

            {read.aside && <p className="si-aside">{read.aside}</p>}

            {read.words.length > 0 && (
              <button
                type="button"
                className="si-words-toggle"
                aria-expanded={wordsOpen}
                onClick={() => setShowWords(!wordsOpen)}
              >
                {wordsOpen
                  ? 'Hide what the app said'
                  : `What the app said (${read.words.length} line${read.words.length === 1 ? '' : 's'})`}
              </button>
            )}

            {wordsOpen && (
              <div className="si-transcript" ref={transcriptRef}>
                {(view?.lines ?? []).map((line, i) => (
                  <div className="si-line" key={i}>{line}</div>
                ))}
                {running && <div className="si-line si-line--live">…</div>}
              </div>
            )}

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
                  placeholder="Paste the code the app asked for"
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
                  // A stop the daemon refuses used to leave this button reading
                  // "Stopping" for the rest of the modal's life while the login
                  // command was still running, and the next attempt then met a
                  // sign-in already in progress with nothing explaining it.
                  stopSignIn(harnessId, deviceId)
                    .catch((err) => setError(errorCopy(err)))
                    .finally(() => setStopping(false))
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

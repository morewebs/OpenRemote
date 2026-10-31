// Pairing a phone by QR code, on the Mobile view.
//
// The daemon does the real work: it holds the one live code, opens a socket
// on the local network while a code is on screen or a phone is paired, and
// keeps the phone records. This component asks for a code, draws it, watches
// for a phone to arrive, and lists the phones that already did - so it needs
// no account, no Cloud, and no network beyond this computer and the phone.
//
// The code arrives blurred. A camera pointed at the screen is how this screen
// is meant to be used, so a code drawn sharp the moment the view opens is a
// code anyone else in the room can read too: one click on it shows the code, a
// second click hides it, and the address beside it stays masked while it is
// hidden.

import { useEffect, useRef, useState } from 'react'
import { DeviceMobile } from '@phosphor-icons/react'
import { maskedAddress, pairedLabel, pairingApi, pairingError, phoneName, remainingLabel } from './pairing.js'
import { qrRows } from './qr.js'
import { useArmedRow } from './use-armed-row.js'
import './pairing.css'

const TICK_MS = 1000
const POLL_MS = 3000

// Four modules of quiet zone, as the standard asks: a scanner cannot find
// the corners without it.
const QUIET = 4

function QrCard({ url, blurred }) {
  let rows = null
  let failed = false
  try {
    rows = qrRows(url)
  } catch {
    failed = true
  }
  if (failed || !rows) {
    // Honest fallback: the address below the code is always readable.
    return <p className="qr-note">This address is too long to draw as a code. Open it on the phone instead.</p>
  }
  const side = rows.length + QUIET * 2
  const dark = []
  rows.forEach((row, y) => {
    row.forEach((module, x) => {
      if (module) dark.push(`M${x + QUIET},${y + QUIET}h1v1h-1z`)
    })
  })
  return (
    <svg
      className={blurred ? 'qr-code is-blurred' : 'qr-code'}
      viewBox={`0 0 ${side} ${side}`}
      role="img"
      aria-label="Pairing code"
    >
      <rect width={side} height={side} fill="#ffffff" />
      <path d={dark.join('')} fill="#0a0c0f" shapeRendering="crispEdges" />
    </svg>
  )
}

export default function PairingSection() {
  const api = useRef(null)
  if (api.current === null) api.current = pairingApi()
  // The live code, exactly as the daemon minted it; null when nothing is on
  // screen.
  const [live, setLive] = useState(null)
  // Whether that code is drawn sharp. The code is live either way: this is a
  // question about the screen, not about the pairing.
  const [shown, setShown] = useState(false)
  // The code that was on screen ran out. The tile says so rather than offer
  // what looks like a first attempt.
  const [expired, setExpired] = useState(false)
  // null until the first list arrives, so "no phones" is never a guess.
  const [phones, setPhones] = useState(null)
  const [joined, setJoined] = useState(null)
  const [error, setError] = useState(null)
  // Which paired phone is waiting on a second press. Removing one drops that
  // phone's credential and its access to this computer, and it used to happen on a
  // single click: the person paired the phone in order to find it, and a misclick
  // cut it off with no undo. The console's usual two-step, safe answer first.
  const [confirmRevoke, setConfirmRevoke] = useState(null)
  const revokeWatch = useArmedRow(confirmRevoke, setConfirmRevoke)
  // The daemon's own complaint about the pairing port (it could not bind at
  // boot with a phone already paired), which no retry here caused.
  const [notice, setNotice] = useState(null)
  const [busy, setBusy] = useState(false)
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    let stopped = false
    api.current
      .phones()
      .then((answer) => {
        if (!stopped) {
          setPhones(answer.phones ?? [])
          setNotice(answer.notice ?? null)
        }
      })
      .catch((err) => {
        // The read failing says nothing about the list: an empty one here
        // would draw "nothing is listening on your network" under the error
        // that says the read failed, which the daemon never told us.
        if (!stopped) setError(pairingError(err))
      })
    return () => {
      stopped = true
    }
  }, [])

  // While a code is on screen: count it down, and watch for the phone. A
  // phone that pairs uses the code up, so the arrival is the end of the
  // countdown rather than a second thing to do.
  useEffect(() => {
    if (!live) return undefined
    const before = phones?.length ?? 0
    const tick = setInterval(() => setNow(Date.now()), TICK_MS)
    const poll = setInterval(async () => {
      try {
        const answer = await api.current.phones()
        const list = answer.phones ?? []
        if (list.length > before) {
          const fresh = list.find((phone) => !(phones ?? []).some((known) => known.id === phone.id))
          setJoined(fresh ? phoneName(fresh.name) : 'A phone')
          setPhones(list)
          setLive(null)
          setShown(false)
        }
      } catch {
        // A poll that fails changes nothing: the code is still on screen,
        // and the countdown still ends it.
      }
    }, POLL_MS)
    return () => {
      clearInterval(tick)
      clearInterval(poll)
    }
  }, [live, phones])

  // The clock runs out: the daemon has already closed the socket, and the
  // card goes away with it.
  useEffect(() => {
    if (!live) return
    if (remainingLabel(live.expires_at, now) !== 'Expired') return
    setLive(null)
    setShown(false)
    setExpired(true)
  }, [live, now])

  const start = async () => {
    setBusy(true)
    setError(null)
    setJoined(null)
    setExpired(false)
    setShown(false)
    try {
      const mint = await api.current.start()
      setNow(Date.now())
      setLive(mint)
      if (Array.isArray(mint.phones)) setPhones(mint.phones)
      // The daemon opened the port to answer this, so a boot-time complaint
      // about the port is no longer true.
      setNotice(null)
    } catch (err) {
      setError(pairingError(err))
    } finally {
      setBusy(false)
    }
  }

  // The view opens with a code already on screen and blurred: pairing a phone
  // should not need a button press first, and a code nobody has read yet costs
  // nothing. It closes itself when the countdown ends, and leaving this view
  // lets it run out rather than leaving a socket open behind us.
  useEffect(() => {
    start()
  }, [])

  const stop = async () => {
    setBusy(true)
    setError(null)
    try {
      const answer = await api.current.cancel()
      if (Array.isArray(answer.phones)) setPhones(answer.phones)
      setNotice(answer.notice ?? null)
      // Only a cancel the daemon confirmed takes the card down: if it failed,
      // the code is still live there, and hiding it would hide that.
      setLive(null)
      setShown(false)
      setExpired(false)
    } catch (err) {
      setError(pairingError(err))
    } finally {
      setBusy(false)
    }
  }

  const revoke = async (id) => {
    setError(null)
    try {
      const answer = await api.current.revoke(id)
      setPhones(answer.phones ?? [])
      setNotice(answer.notice ?? null)
    } catch (err) {
      // Already gone is the outcome the click wanted.
      if (err?.status === 404) {
        try {
          const answer = await api.current.phones()
          setPhones(answer.phones ?? [])
        } catch {
          setPhones((list) => (list ?? []).filter((phone) => phone.id !== id))
        }
        return
      }
      setError(pairingError(err))
    }
  }

  // What the tile says when there is no code on screen: one is being minted,
  // or the last one ran out, or the daemon refused and the sentence underneath
  // says why.
  const emptyLabel = busy
    ? 'Preparing a code'
    : expired
      ? 'The code expired. Click for a new one.'
      : 'Click for a code'

  return (
    <div className="qr-pair">
      <h2 className="pn-section">Pair a phone</h2>
      <p className="pn-lead">Scan this with your phone's camera. No account needed.</p>

      {joined && <p className="qr-note qr-joined">{joined} is paired.</p>}

      {live ? (
        <div className="qr-card">
          <div className="qr-tile-wrap">
            <button
              type="button"
              className="qr-tile"
              aria-pressed={shown}
              aria-label={shown ? 'Hide the pairing code' : 'Show the pairing code'}
              onClick={() => setShown((on) => !on)}
            >
              <QrCard url={live.url} blurred={!shown} />
            </button>
            <p className="qr-hint">{shown ? 'Click the code to hide it' : 'Click the code to show it'}</p>
          </div>
          <div className="qr-side">
            <p className="qr-wait">
              <span className="dot dot--waiting" />
              Waiting for a phone
            </p>
            <p className="qr-clock">{remainingLabel(live.expires_at, now)}</p>
            <p className="qr-address">{shown ? live.url : maskedAddress(live.url)}</p>
            <p className="qr-note">
              Not encrypted, and your firewall may need to allow OpenRemote. Same network only.
            </p>
            <button className="pn-btn" onClick={stop} disabled={busy}>
              Stop showing
            </button>
          </div>
        </div>
      ) : (
        <button type="button" className="qr-tile qr-tile--empty" onClick={start} disabled={busy}>
          <span className="qr-blank" aria-hidden="true" />
          <span className="qr-hint">{emptyLabel}</span>
        </button>
      )}

      {error && <p className="qr-error">{error}</p>}
      {notice && !error && <p className="qr-notice">{notice}</p>}

      {phones !== null && phones.length > 0 && (
        <ul className="qr-phones">
          {phones.map((phone) => (
            <li key={phone.id} className="qr-phone">
              <span className="qr-phone-icon">
                <DeviceMobile size={18} weight="duotone" />
              </span>
              <span className="qr-phone-name pv">{phoneName(phone.name)}</span>
              <span className="qr-phone-when">{pairedLabel(phone, now)}</span>
              {confirmRevoke === phone.id ? (
                <>
                  <button className="pn-btn" onClick={() => setConfirmRevoke(null)}>
                    Keep it paired
                  </button>
                  <button
                    className="pn-btn pn-btn--danger"
                    ref={revokeWatch.buttonRef}
                    onBlur={revokeWatch.onBlur}
                    onClick={() => {
                      setConfirmRevoke(null)
                      revoke(phone.id)
                    }}
                  >
                    Remove
                  </button>
                </>
              ) : (
                <button className="pn-btn" onClick={() => setConfirmRevoke(phone.id)}>
                  Remove
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {phones !== null && phones.length === 0 && (
        <p className="pn-empty">No phones paired.</p>
      )}
    </div>
  )
}
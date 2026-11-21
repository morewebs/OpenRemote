// What Cloud owes the user right where they work: a device that joined
// their account (any sign-in adds one, so "Not you?" is their check), and
// this computer being made a machine from another device. Each notice is
// shown once and says what it can undo.

import { useEffect, useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Bell, CaretDown, CaretRight, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { kindLabel, madeMachineElsewhere, newDevices, platformLabel, signedIn } from './cloud.js'
import { errorCopy } from './error-copy.js'
import './notices.css'

function load(key) {
  try {
    return JSON.parse(localStorage.getItem(key) ?? 'null')
  } catch {
    return null
  }
}

function save(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* storage unavailable */
  }
}

const lastActiveFormat = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

export default function CloudNotices({ reviewTarget }) {
  const { cloud, devices, thisDevice, removeDevice, setDeviceKind } = useConsole()
  const account = cloud?.account?.id
  const knownKey = `openremote-known-devices:${account}`
  const kindKey = `openremote-last-kind:${thisDevice}`
  const [known, setKnown] = useState(null)
  const [lastKind, setLastKind] = useState(null)
  const [reviewing, setReviewing] = useState(null)
  const [confirming, setConfirming] = useState(null)
  // The device whose removal is in flight. The confirm's own button used to
  // fire twice on a double click, and the second call failed with "no such
  // device" and painted that error over a removal that had worked.
  const [removing, setRemoving] = useState(null)
  const [deviceError, setDeviceError] = useState(null)
  const [error, setError] = useState(null)
  const reviewId = useId()
  const headingRef = useRef(null)
  const revokeRef = useRef(null)
  const ready = signedIn(cloud) && account && thisDevice && (devices ?? []).length > 0
  const me = (devices ?? []).find((d) => d.id === thisDevice)

  const settleKind = (kind) => {
    save(kindKey, kind)
    setLastKind(kind)
  }

  // The first look at an account is the baseline: nothing is new yet.
  useEffect(() => {
    if (!ready) return
    let saved = load(knownKey)
    if (!saved) {
      saved = devices.map((d) => d.id)
      save(knownKey, saved)
    }
    setKnown(saved)
    let kind = load(kindKey)
    if (!kind && me) {
      kind = me.kind
      save(kindKey, kind)
    }
    setLastKind(kind)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, knownKey, kindKey])

  // A kind change that is not news (made here, or reverted) is remembered
  // in an effect, never in the render body: a render can be thrown away
  // (StrictMode double-run, a concurrent update landing mid-render), and
  // a storage write from a render nobody committed to must not stand as
  // the last word. The baseline effect above establishes `lastKind`
  // first; until it has, there is nothing to reconcile against. The
  // `lastKind` dep re-runs this once the baseline lands, so the two never
  // race past each other.
  useEffect(() => {
    if (!me || lastKind == null || me.kind === lastKind) return
    if (madeMachineElsewhere(me.kind, lastKind, load(`openremote-kind-set-here:${thisDevice}`))) return
    // Changed here, or back: nothing to say, just remember it.
    settleKind(me.kind)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [me?.kind, lastKind])

  if (!ready || !known) return null

  const acknowledge = (id) => {
    const next = [...known, id]
    save(knownKey, next)
    setKnown(next)
    setReviewing(null)
    setConfirming(null)
  }

  const fresh = newDevices(devices, known, thisDevice)
  const madeMachine = me && madeMachineElsewhere(me.kind, lastKind, load(`openremote-kind-set-here:${thisDevice}`))
  if (fresh.length === 0 && !madeMachine && !error) return null

  const deviceReview = fresh.length > 0 && (
    <div className="cn-card">
      <div className="cn-card-icon" aria-hidden="true"><Bell size={16} weight="duotone" /></div>
      <div className="cn-card-copy">
        <div className="cn-card-title" role="status">
          {fresh.length} new Cloud {fresh.length === 1 ? 'device' : 'devices'}
        </div>
        <button type="button" className="cn-review-trigger" popoverTarget={reviewId}>
          Review devices <CaretRight size={11} aria-hidden="true" />
        </button>
      </div>
      <section
        className="cn-panel"
        id={reviewId}
        popover="auto"
        aria-labelledby={`${reviewId}-title`}
        onToggle={(event) => {
          if (event.newState !== 'open') return
          if (removing === null) {
            setReviewing(null)
            setConfirming(null)
            setDeviceError(null)
          }
          headingRef.current?.focus({ preventScroll: true })
        }}
      >
        <header className="cn-panel-header">
          <h2 id={`${reviewId}-title`} tabIndex={-1} ref={headingRef}>New Cloud devices</h2>
          <button type="button" className="cn-panel-close" aria-label="Close device review" popoverTarget={reviewId} popoverTargetAction="hide">
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        <ul className="cn-device-list">
          {fresh.map((device, index) => {
            const deviceLabel = `${reviewId}-device-${index}`
            const detailId = `${deviceLabel}-details`
            const expanded = reviewing === device.id
            const platform = device.kind === 'web' ? null : platformLabel(device.platform)
            const detail = [kindLabel(device), platform, `ID ${device.id.slice(-6)}`].filter(Boolean).join(' · ')
            const lastSeen = device.online ? 'Online now' : device.last_seen_at
              ? `Last active ${lastActiveFormat.format(device.last_seen_at * 1000)}`
              : 'Offline'
            return (
              <li key={device.id} className="cn-device">
                <button
                  type="button"
                  className="cn-device-toggle"
                  aria-expanded={expanded}
                  aria-controls={detailId}
                  disabled={removing !== null}
                  onClick={() => {
                    setReviewing(expanded ? null : device.id)
                    setConfirming(null)
                    setDeviceError(null)
                  }}
                >
                  <span className="cn-device-summary">
                    <span className="cn-device-name" id={deviceLabel}>{device.name}</span>
                    <span className="cn-device-meta">{lastSeen}</span>
                  </span>
                  <CaretDown size={13} aria-hidden="true" />
                </button>
                <div className="cn-device-details" id={detailId} hidden={!expanded}>
                  {expanded && <div className="cn-device-meta">{detail}</div>}
                  {expanded && (confirming === device.id ? (
                    <div className="cn-confirm">
                      <p>This device will lose access to your Cloud machines and chats.</p>
                      <div className="cn-device-actions">
                        <button type="button" className="cn-btn" autoFocus disabled={removing !== null} onClick={() => {
                          setConfirming(null)
                          setDeviceError(null)
                          requestAnimationFrame(() => revokeRef.current?.focus())
                        }}>
                          Keep access
                        </button>
                        <button
                          type="button"
                          className="cn-btn cn-danger"
                          aria-describedby={deviceLabel}
                          disabled={removing !== null}
                          onClick={async () => {
                            setRemoving(device.id)
                            setDeviceError(null)
                            try {
                              await removeDevice(device.id)
                              if (headingRef.current?.closest(':popover-open')) headingRef.current.focus()
                              acknowledge(device.id)
                            } catch (err) {
                              setDeviceError({ id: device.id, text: errorCopy(err) })
                            } finally {
                              setRemoving(null)
                            }
                          }}
                        >
                          {removing === device.id ? 'Revoking…' : 'Revoke access'}
                        </button>
                      </div>
                      {deviceError?.id === device.id && <p className="cn-device-error" role="alert">{deviceError.text}</p>}
                    </div>
                  ) : (
                    <div className="cn-device-actions">
                      <button type="button" className="cn-btn cn-recognize" aria-describedby={deviceLabel} disabled={removing !== null} onClick={() => {
                        headingRef.current?.focus()
                        acknowledge(device.id)
                      }}>
                        I recognize this
                      </button>
                      <button type="button" className="cn-btn cn-revoke" ref={revokeRef} aria-describedby={deviceLabel} disabled={removing !== null} onClick={() => {
                        setConfirming(device.id)
                        setDeviceError(null)
                      }}>
                        Revoke access…
                      </button>
                    </div>
                  ))}
                </div>
              </li>
            )
          })}
        </ul>
      </section>
    </div>
  )

  return (
    <>
      {deviceReview && (reviewTarget ? createPortal(deviceReview, reviewTarget) : <div className="cn-stack">{deviceReview}</div>)}
      {(madeMachine || error) && (
        <div className="cn-stack" role="status">
          {madeMachine && (
            <div className="cn-row">
              <span className="cn-text">
                This computer was made a machine: your other devices can run chats here while OpenRemote is open.
              </span>
              <button
                type="button"
                className="cn-btn"
                onClick={async () => {
                  setError(null)
                  try {
                    await setDeviceKind(thisDevice, 'desktop')
                    settleKind('desktop')
                  } catch (err) {
                    setError(errorCopy(err))
                  }
                }}
              >
                Undo
              </button>
              <button type="button" className="cn-btn" onClick={() => settleKind('machine')}>
                OK
              </button>
            </div>
          )}
          {error && (
            <div className="cn-row">
              <span className="cn-text">{error}</span>
              <button type="button" className="cn-btn" onClick={() => setError(null)}>
                OK
              </button>
            </div>
          )}
        </div>
      )}
    </>
  )
}

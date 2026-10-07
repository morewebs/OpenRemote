// What Cloud owes the user right where they work: a device that joined
// their account (any sign-in adds one, so "Not you?" is their check), and
// this computer being made a machine from another device. Each notice is
// shown once and says what it can undo.

import { useEffect, useState } from 'react'
import { useConsole } from './state/console.jsx'
import { madeMachineElsewhere, newDevices, signedIn } from './cloud.js'
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

export default function CloudNotices() {
  const { cloud, devices, thisDevice, removeDevice, setDeviceKind } = useConsole()
  const account = cloud?.account?.id
  const knownKey = `openremote-known-devices:${account}`
  const kindKey = `openremote-last-kind:${thisDevice}`
  const [known, setKnown] = useState(null)
  const [lastKind, setLastKind] = useState(null)
  const [confirming, setConfirming] = useState(null)
  const [error, setError] = useState(null)
  const ready = signedIn(cloud) && account && thisDevice && (devices ?? []).length > 0
  const me = (devices ?? []).find((d) => d.id === thisDevice)

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

  if (!ready || !known) return null

  const acknowledge = (id) => {
    const next = [...known, id]
    save(knownKey, next)
    setKnown(next)
    setConfirming(null)
  }

  const settleKind = (kind) => {
    save(kindKey, kind)
    setLastKind(kind)
  }

  const fresh = newDevices(devices, known, thisDevice)
  const madeMachine = me && madeMachineElsewhere(me.kind, lastKind, load(`openremote-kind-set-here:${thisDevice}`))
  if (me && me.kind !== lastKind && !madeMachine) {
    // Changed here, or back: nothing to say, just remember it.
    settleKind(me.kind)
  }
  if (fresh.length === 0 && !madeMachine && !error) return null

  return (
    <div className="cn-stack" role="status">
      {fresh.map((device) => (
        <div key={device.id} className="cn-row">
          <span className="cn-text">
            {confirming === device.id
              ? `Remove ${device.name}? It loses access to your machines and chats.`
              : `New device: ${device.name} joined your Cloud. Not you?`}
          </span>
          {confirming === device.id ? (
            <>
              <button
                type="button"
                className="cn-btn primary"
                onClick={async () => {
                  setError(null)
                  try {
                    await removeDevice(device.id)
                    acknowledge(device.id)
                  } catch (err) {
                    setError(err.message ?? String(err))
                  }
                }}
              >
                Remove
              </button>
              <button type="button" className="cn-btn" onClick={() => setConfirming(null)}>
                Keep
              </button>
            </>
          ) : (
            <>
              <button type="button" className="cn-btn" onClick={() => setConfirming(device.id)}>
                Remove
              </button>
              <button type="button" className="cn-btn" onClick={() => acknowledge(device.id)}>
                It's mine
              </button>
            </>
          )}
        </div>
      ))}
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
                setError(err.message ?? String(err))
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
  )
}

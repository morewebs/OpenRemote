// The browser's front door: sign in to moreweb, then pick one of the user's
// machines, and the device view takes over from there.
//
// Nothing here runs a harness or holds a key for one. Signing in this browser
// takes a seat in the account as a device of its own, and the link that opens
// afterwards carries requests to the machine the person picked. This page's
// job is to establish that link and nothing else.

import { useCallback, useEffect, useRef, useState } from 'react'
import { ArrowRight, Globe, WarningCircle } from '@phosphor-icons/react'
import { SplashMark } from '../Splash.jsx'
import { openExternal } from '../platform.js'
import {
  authorizeUrl,
  challengeFor,
  cloudTarget,
  redirectFor,
  stateFor,
  tokenBody,
  tokenUrl,
  verifierFor,
} from './cloud.js'
import { machineLine, machinesOf, pickRefusal } from './devices.js'
import { browserDeviceName, loadPeer } from './peer.js'
import { clearPending, clearTokens, loadPending, loadTokens, savePending, saveTokens } from './session.js'
// The Cloud surface's own sky, which this page and the device view both sit
// in. It lives beside the Cloud views, not here.
import '../cloudmode.css'
import './web-signin.css'

const LINKS_MISSING =
  'The secure link to your machines is not part of this build yet. Signing in works, but this page cannot reach a computer until that link ships.'

// What one refused answer says, in the words a person can use. The gateway
// answers OAuth refusals as a form body and API refusals as JSON, so both are
// read here rather than guessed at call sites.
function refusal(payload, fallback) {
  if (!payload || typeof payload !== 'object') return fallback
  const said = payload.error_description ?? payload.message ?? payload.error
  return typeof said === 'string' && said.trim() ? said.trim() : fallback
}

export default function SignInSurface({ onLinked }) {
  const [step, setStep] = useState('checking')
  const [machines, setMachines] = useState([])
  const [problem, setProblem] = useState(null)
  const [linking, setLinking] = useState(true)
  const started = useRef(false)
  const target = useRef(cloudTarget({ search: window.location.search, env: window.__OPENREMOTE_CLOUD__ }))
  const redirect = redirectFor(window.location)

  const listMachines = useCallback(
    async (tokens, peer) => {
      let response
      try {
        response = await fetch(`${target.current.api}/devices`, {
          headers: { Authorization: `Bearer ${tokens.access}` },
        })
      } catch {
        setProblem('The gateway could not be reached. Check this computer is online, then try again.')
        setStep('error')
        return
      }
      if (response.status === 401) {
        clearTokens()
        setProblem('Your sign-in expired. Sign in with moreweb again.')
        setStep('intro')
        return
      }
      if (!response.ok) {
        setProblem(`The gateway answered ${response.status} when asked for your devices.`)
        setStep('error')
        return
      }
      const payload = await response.json().catch(() => null)
      setMachines(machinesOf(payload))
      setLinking(Boolean(peer))
      setStep('devices')
    },
    [],
  )

  // One sign-in at a time: the page either came back from the gateway with a
  // code, has tokens from an earlier visit, or starts at the door.
  useEffect(() => {
    if (started.current) return
    started.current = true
    ;(async () => {
      const peer = await loadPeer()
      setLinking(Boolean(peer))
      const params = new URLSearchParams(window.location.search)
      const denied = params.get('error')
      if (denied) {
        clearPending()
        setProblem(refusal(Object.fromEntries(params), 'The sign-in was cancelled.'))
        setStep('intro')
        return
      }
      const code = params.get('code')
      const state = params.get('state')
      if (code && state) {
        const pending = loadPending()
        clearPending()
        if (!pending || pending.state !== state) {
          setProblem('That sign-in did not come from this page. Sign in again to start a fresh one.')
          setStep('intro')
          return
        }
        // The gateway's return address carries the code and the state and
        // nothing else, so this page's own `?api=` and `?issuer=` are gone by
        // now. The sign-in remembered the gateway it started at, and that is
        // the one to finish with; without this a page pointed at another
        // backend would exchange its code against the default one.
        if (pending.api && pending.issuer) {
          target.current = { api: pending.api, issuer: pending.issuer }
        }
        let response
        try {
          response = await fetch(tokenUrl(target.current.issuer), {
            method: 'POST',
            headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
            body: tokenBody({ code, redirect: pending.redirect || redirect, verifier: pending.verifier }),
          })
        } catch {
          setProblem('The gateway could not be reached to finish the sign-in. Try again.')
          setStep('intro')
          return
        }
        const payload = await response.json().catch(() => null)
        if (!response.ok || typeof payload?.access_token !== 'string') {
          setProblem(refusal(payload, 'The gateway refused the sign-in. Try again.'))
          setStep('intro')
          return
        }
        const tokens = {
          access: payload.access_token,
          refresh: typeof payload.refresh_token === 'string' ? payload.refresh_token : null,
          expiresAt: Number.isFinite(payload.expires_in) ? Date.now() + payload.expires_in * 1000 : 0,
        }
        saveTokens(tokens)
        // The browser takes a seat in the account as a device, so the
        // machines' allowlists can recognise it. The peer owns the key pair
        // and the registration; a build without it keeps the tokens and says
        // what is missing.
        if (peer) {
          try {
            await peer.ensureDevice({
              api: target.current.api,
              accessToken: tokens.access,
              name: browserDeviceName(),
            })
          } catch {
            setProblem('This browser could not join your account as a device. Try signing in again.')
            setStep('error')
            return
          }
        }
        await listMachines(tokens, peer)
        return
      }
      const tokens = loadTokens()
      if (tokens) {
        await listMachines(tokens, peer)
        return
      }
      setStep('intro')
    })()
  }, [listMachines, redirect])

  const signIn = async () => {
    setProblem(null)
    try {
      const verifier = verifierFor()
      const state = stateFor()
      savePending({
        verifier,
        state,
        redirect,
        api: target.current.api,
        issuer: target.current.issuer,
      })
      const challenge = await challengeFor(verifier)
      const url = authorizeUrl({ issuer: target.current.issuer, redirect, state, challenge })
      // A full page navigation: the gateway returns to this page with the
      // code, and the reader above finishes the exchange.
      window.location.assign(url)
    } catch {
      setProblem('This browser could not start the sign-in. Check that it allows storage for this page.')
    }
  }

  const signOut = () => {
    clearTokens()
    setMachines([])
    setProblem(null)
    setStep('intro')
  }

  const pick = (machine) => {
    // The link first, then the machine: a build without the link cannot reach
    // any machine, and saying which of the two is missing is the difference
    // between a build to update and a machine to wake.
    if (!linking) {
      setProblem(LINKS_MISSING)
      return
    }
    const refusalFor = pickRefusal(machine)
    if (refusalFor) setProblem(refusalFor)
    else onLinked(machine)
  }

  return (
    <div className="cloudmode web-signin">
      <div className="cm-horizon" aria-hidden="true" />
      <main className="cm-panel web-signin-panel" aria-live="polite">
        <SplashMark className="web-signin-mark" />
        <h1 className="cm-title">OpenRemote</h1>
        <p className="cm-body">
          This is OpenRemote in the browser. The chats stay on your own computers; this page is a window
          onto them.
        </p>

        {step === 'checking' && <p className="cm-body web-signin-quiet">Checking this browser's sign-in.</p>}

        {step === 'intro' && (
          <div className="cm-actions">
            <button type="button" className="cm-back primary" onClick={signIn}>
              Sign in with moreweb
              <ArrowRight size={13} weight="bold" />
            </button>
          </div>
        )}

        {step === 'devices' && (
          <>
            <p className="cm-body web-signin-sub">
              Choose the computer to reach. Its chats, its files and its harnesses stay there.
            </p>
            {!linking && <p className="cm-error web-signin-honest">{LINKS_MISSING}</p>}
            {machines.length === 0 ? (
              <p className="cm-body">
                No machines in this account yet. Install the OpenRemote agent on a computer and join it
                with a code from that machine's list, then come back to this page.
              </p>
            ) : (
              <ul className="web-signin-machines">
                {machines.map((machine) => {
                  const refusalFor = pickRefusal(machine)
                  return (
                    <li key={machine.id} className="web-signin-machine">
                      <span className="web-signin-machine-mark" aria-hidden="true">
                        <Globe size={16} />
                      </span>
                      <span className="web-signin-machine-body">
                        <span className="web-signin-machine-name">{machine.name}</span>
                        <span
                          className={`web-signin-machine-line${machine.online ? ' is-online' : ''}`}
                        >
                          {machineLine(machine)}
                        </span>
                      </span>
                      <button
                        type="button"
                        className="cm-back"
                        onClick={() => pick(machine)}
                        title={refusalFor ?? undefined}
                      >
                        Open
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
            <div className="cm-actions">
              <button type="button" className="cm-back" onClick={signOut}>
                Sign out
              </button>
              <button
                type="button"
                className="cm-back"
                onClick={() => openExternal('https://moreweb.space').catch(() => {})}
              >
                moreweb.space
              </button>
            </div>
          </>
        )}

        {problem && (
          <p className="cm-error web-signin-problem">
            <WarningCircle size={14} /> {problem}
          </p>
        )}
      </main>
    </div>
  )
}

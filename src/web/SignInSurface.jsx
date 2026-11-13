// The browser's front door: sign in to moreweb, then pick one of the user's
// machines, and the device view takes over from there.
//
// Nothing here runs a harness or holds a key for one. Signing in this browser
// takes a seat in the account as a device of its own, and the link that opens
// afterwards carries requests to the machine the person picked. This page's
// job is to establish that link and nothing else.

import { useCallback, useEffect, useRef, useState } from 'react'
import { Desktop, Info, LockKey, UserCircle, WarningCircle } from '@phosphor-icons/react'
import { SplashMark } from '../Splash.jsx'
import { openExternal } from '../platform.js'
import { platformLabel } from '../cloud.js'
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
import LoadingStatus from './LoadingStatus.jsx'
import { accountFetch } from './account-session.js'
import { clearPending, clearTokens, loadPending, loadTokens, savePending, saveTokens } from './session.js'
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

// This thumbnail identifies a computer without fetching any desktop content.
function MachineDisplay() {
  return (
    <span className="web-machine-display" aria-hidden="true">
      <svg viewBox="0 0 80 64" fill="none">
        <rect x="4" y="4" width="72" height="46" rx="5" stroke="currentColor" strokeWidth="1.5" />
        <rect x="9" y="9" width="62" height="36" rx="2" className="web-machine-screen" />
        <path d="M40 50v10m-14 0h28" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
      <SplashMark className="web-machine-mark" />
    </span>
  )
}

export default function SignInSurface({ onLinked }) {
  const [step, setStep] = useState('checking')
  const [machines, setMachines] = useState([])
  const [account, setAccount] = useState(null)
  const [view, setView] = useState('machines')
  const [problem, setProblem] = useState(null)
  const [linking, setLinking] = useState(true)
  // One sign-in at a time: a second press while the first is navigating would
  // save a second verifier and strand the callback that comes back.
  const [starting, setStarting] = useState(false)
  const [connecting, setConnecting] = useState(null)
  const started = useRef(false)
  const target = useRef(cloudTarget({ search: window.location.search, env: window.__OPENREMOTE_CLOUD__ }))
  const redirect = redirectFor(window.location)

  const listMachines = useCallback(
    async (_tokens, peer) => {
      let response
      try {
        response = await accountFetch(target.current, '/devices')
      } catch (error) {
        setProblem(error?.code === 'session_expired' ? error.message : 'OpenRemote Cloud could not be reached. Your sign-in is saved. Check your connection and try again.')
        setStep(error?.code === 'session_expired' ? 'intro' : 'error')
        return
      }
      if (response.status === 401) {
        setProblem('Your sign-in could not be verified after renewal. Try signing in with moreweb again.')
        setStep('intro')
        return
      }
      if (!response.ok) {
        setProblem(`OpenRemote Cloud could not list your devices (error ${response.status}). Try again in a minute.`)
        setStep('error')
        return
      }
      const payload = await response.json().catch(() => null)
      setMachines(machinesOf(payload))
      setAccount(payload?.account ?? null)
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
        const pending = loadPending(state)
        if (!pending) {
          // A callback reloaded after it already finished has no pending
          // entry. The tokens it saved are the real answer; only a page with
          // neither a pending sign-in nor tokens is somebody else's.
          const tokens = loadTokens()
          if (tokens) {
            await listMachines(tokens, peer)
            return
          }
          setProblem('That sign-in did not come from this page. Sign in again to start a fresh one.')
          setStep('intro')
          return
        }
        clearPending(state)
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
          setProblem('moreweb could not be reached to finish the sign-in. Try again.')
          setStep('intro')
          return
        }
        const payload = await response.json().catch(() => null)
        if (!response.ok || typeof payload?.access_token !== 'string') {
          setProblem(refusal(payload, 'moreweb refused the sign-in. Try again.'))
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
    if (starting) return
    setStarting(true)
    setProblem(null)
    try {
      const verifier = verifierFor()
      const state = stateFor()
      const challenge = await challengeFor(verifier)
      const saved = savePending({
        verifier,
        state,
        redirect,
        api: target.current.api,
        issuer: target.current.issuer,
      })
      if (!saved) {
        setProblem(
          'This browser could not remember the sign-in. Check that it allows storage for this page.',
        )
        setStarting(false)
        return
      }
      const url = authorizeUrl({ issuer: target.current.issuer, redirect, state, challenge })
      // A full page navigation: the gateway returns to this page with the
      // code, and the reader above finishes the exchange.
      window.location.assign(url)
    } catch {
      setProblem('This browser could not start the sign-in. Check that it allows storage for this page.')
      setStarting(false)
    }
  }

  const signOut = () => {
    clearTokens()
    setMachines([])
    setAccount(null)
    setView('machines')
    setProblem(null)
    setStep('intro')
  }

  const pick = async (machine) => {
    if (connecting) return
    // The link first, then the machine: a build without the link cannot reach
    // any machine, and saying which of the two is missing is the difference
    // between a build to update and a machine to wake.
    if (!linking) {
      setProblem(LINKS_MISSING)
      return
    }
    const refusalFor = pickRefusal(machine)
    if (refusalFor) setProblem(refusalFor)
    else {
      setProblem(null)
      setConnecting(machine.id)
      try { await onLinked(machine) } finally { setConnecting(null) }
    }
  }

  return (
    <div className="web-signin">
      <aside className="web-signin-sidebar">
        <div className="web-signin-brand">
          <SplashMark className="web-signin-mark" />
          <span>OpenRemote</span>
        </div>
        <nav className="web-signin-nav" aria-label="Main navigation">
          <button type="button" aria-current={view === 'machines' ? 'page' : undefined} onClick={() => setView('machines')}>
            <Desktop size={18} aria-hidden="true" />Machines
          </button>
          <button type="button" aria-current={view === 'account' ? 'page' : undefined} onClick={() => setView('account')}>
            <UserCircle size={18} aria-hidden="true" />Account
          </button>
        </nav>
        <button type="button" className="web-signin-link web-signin-site" onClick={() => openExternal('https://moreweb.space').catch(() => {})}>moreweb.space</button>
      </aside>

      <div className="web-signin-content">
      <main className="web-signin-main">
        <section className="web-signin-panel" aria-live="polite" aria-labelledby="web-signin-heading">
          {step === 'devices' && view === 'account' ? (
            <div className="web-signin-account">
              <h1 id="web-signin-heading">Account</h1>
              <p>Manage your sign-in on this browser.</p>
              <div className="web-signin-account-surface">
                <div className="web-signin-identity">
                  <span className="web-signin-avatar" aria-hidden="true"><UserCircle size={30} weight="light" /></span>
                  <div>
                    <h2>{typeof account?.email === 'string' ? account.email : 'moreweb account'}</h2>
                    <p>Signed in with moreweb</p>
                  </div>
                </div>
                <div className="web-signin-account-action">
                  <div><h3>Sign out</h3><p>Disconnect this browser from your account.</p></div>
                  <button type="button" className="web-signin-secondary" onClick={signOut}>Sign out</button>
                </div>
              </div>
            </div>
          ) : step === 'devices' ? (
            <>
              <div className="web-signin-panel-heading">
                <div>
                  <h1 id="web-signin-heading">Machines</h1>
                  <p>Computers linked to your account.</p>
                </div>
              </div>
              {!linking && <p className="web-signin-notice">{LINKS_MISSING}</p>}
              {machines.length === 0 ? (
                <div className="web-signin-empty">
                  <MachineDisplay />
                  <h3>Add your first machine</h3>
                  <p>Open OpenRemote on your computer and choose Machines, then Make this computer a machine. A Linux server can be added from dash.moreweb.space instead.</p>
                </div>
              ) : (
                <ul className="web-signin-machines">
                  {machines.map((machine) => {
                    const refusalFor = pickRefusal(machine)
                    return (
                      <li key={machine.id} className={`web-signin-machine${machine.online ? '' : ' is-offline'}`}>
                        <div className="web-signin-machine-row">
                          <MachineDisplay />
                          <div className="web-signin-machine-body">
                            <span className="web-signin-machine-name">{machine.name}</span>
                            {machine.platform && <span className="web-signin-machine-platform">{platformLabel(machine.platform)}</span>}
                          </div>
                          <span className={`web-signin-machine-status${machine.online ? ' is-online' : ''}`}><span aria-hidden="true" />{machineLine(machine)}</span>
                          <button type="button" className="web-signin-open" disabled={Boolean(connecting) || !machine.online || !linking} onClick={() => pick(machine)} title={refusalFor ?? undefined}>
                            {connecting === machine.id ? 'Connecting...' : 'Open'}
                          </button>
                        </div>
                        {!machine.online && <div className="web-signin-machine-note"><Info size={16} aria-hidden="true" /><p>Turn on this computer and start OpenRemote to connect.</p></div>}
                      </li>
                    )
                  })}
                </ul>
              )}
            </>
          ) : (
            <div className="web-signin-door">
              <MachineDisplay />
              <h1 id="web-signin-heading">Sign in to OpenRemote</h1>
              {step === 'checking' ? (
                <LoadingStatus>Checking your sign-in...</LoadingStatus>
              ) : step === 'intro' ? (
                <>
                  <p>Connect to your computers with your moreweb account.</p>
                  <button type="button" className="web-signin-primary" onClick={signIn} disabled={starting}>
                    {starting ? 'Opening sign-in...' : 'Continue with moreweb'}
                  </button>
                  <p className="web-signin-door-note">Your chats and files stay on your computers.</p>
                </>
              ) : (
                <>
                  <p>Your computers could not be loaded. You can try again without signing out.</p>
                  <button type="button" className="web-signin-primary" onClick={async () => {
                    setProblem(null)
                    setStep('checking')
                    await listMachines(loadTokens(), await loadPeer())
                  }}>Try again</button>
                </>
              )}
            </div>
          )}
          {connecting && <LoadingStatus>Connecting to your computer...</LoadingStatus>}
          {problem && (
            <p className="web-signin-problem"><WarningCircle size={17} aria-hidden="true" /> <span>{problem}</span></p>
          )}
        </section>
        {step === 'devices' && view === 'machines' && <p className="web-signin-ownership"><LockKey size={15} aria-hidden="true" />Your chats and files stay on your computers.</p>}
      </main>
      </div>
    </div>
  )
}

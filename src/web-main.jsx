// The web console's entry: sign in, and enter the desktop console's Cloud
// mode. The browser has no daemon and no harnesses; it is a device of the
// account, and every chat runs on a machine the person picked.
//
// The desktop entry is unchanged: there the shell starts the daemon and
// reports its address. Here the page reaches the user's own machines the way
// another device would: through the account's relay, asking only what a peer
// may ask a machine for.

import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import '@fontsource/geist-mono/400.css'
import 'katex/dist/katex.min.css'
// The tokens and the page reset, which the desktop gets from the sidebar's
// sheet and this entry gets on its own: without them every view falls back to
// the browser's serif.
import './base.css'
import './privacy.css'
import Splash from './Splash.jsx'
import LoadingStatus from './web/LoadingStatus.jsx'
import { restoreBootPrefs } from './settings.js'
import { openLinksOutside } from './platform.js'
import { loadPeer, browserDeviceName } from './web/peer.js'
import { cloudTarget } from './web/cloud.js'
import { accountTokens } from './web/account-session.js'
import { clearDevice, clearTokens, loadDevice, loadTokens, saveDevice } from './web/session.js'
import WebShell from './web/WebShell.jsx'
import SignInSurface from './web/SignInSurface.jsx'

// Before first paint: the persisted reduce-motion, density and privacy mode,
// so the page never flashes animated, mis-spaced or readable.
restoreBootPrefs(document, window.location)
openLinksOutside(document)

function WebRoot() {
  const [route, setRoute] = useState({ step: 'booting' })
  const [peer, setPeer] = useState(null)
  const [tokens, setTokens] = useState(null)
  const target = cloudTarget({
    search: window.location.search,
    env: window.__OPENREMOTE_CLOUD__,
  })

  // A machine picked on an earlier visit is worth exactly as much as the link
  // that carried it: without the link, or without the account tokens that got
  // this page that far, the front door is the honest place to stand again.
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const link = await loadPeer()
      if (cancelled) return
      setPeer(link)
      const saved = loadDevice()
      let savedTokens = loadTokens()
      if (!link || !saved || !savedTokens) {
        setRoute({ step: 'signin' })
        return
      }
      try {
        savedTokens = await accountTokens(target)
        await link.ensureDevice({
          api: target.api,
          accessToken: savedTokens.access,
          name: browserDeviceName(),
        })
      } catch {
        if (!cancelled) setRoute({ step: 'signin' })
        return
      }
      if (!cancelled) {
        setTokens(savedTokens)
        setRoute({ step: 'shell', machine: saved })
      }
    })()
    return () => {
      cancelled = true
    }
  }, [target.api, target.issuer])

  const linked = async (machine) => {
    const link = peer ?? (await loadPeer())
    let savedTokens = loadTokens()
    if (!link || !savedTokens) {
      setRoute({ step: 'signin' })
      return
    }
    setPeer(link)
    try {
      savedTokens = await accountTokens(target)
      await link.ensureDevice({
        api: target.api,
        accessToken: savedTokens.access,
        name: browserDeviceName(),
      })
    } catch {
      setRoute({ step: 'signin' })
      return
    }
    saveDevice({ id: machine.id, name: machine.name })
    setTokens(savedTokens)
    setRoute({ step: 'shell', machine })
  }

  const signOut = () => {
    clearTokens()
    clearDevice()
    peer?.close?.()
    setTokens(null)
    setRoute({ step: 'signin' })
  }

  if (route.step === 'shell' && peer && tokens) {
    return (
      <StrictMode>
        <WebShell
          machine={route.machine}
          peer={peer}
          tokens={tokens}
          target={target}
          onSignOut={signOut}
        />
      </StrictMode>
    )
  }
  return (
    <StrictMode>
      {route.step === 'booting' && <LoadingStatus full>Restoring your workspace...</LoadingStatus>}
      {route.step === 'signin' && <SignInSurface onLinked={linked} />}
      <Splash />
    </StrictMode>
  )
}

createRoot(document.getElementById('root')).render(<WebRoot />)

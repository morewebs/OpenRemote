// The web console's entry: sign in, pick a machine, and read that machine's
// synced chats over the encrypted link.
//
// The desktop entry is unchanged: there the shell starts the daemon and reports
// its address. Here there is no daemon on this computer at all, so the page
// reaches the user's own machine the way another device would: through the
// account's relay, asking only the two routes a peer may ask a machine for.

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
import { restoreBootPrefs } from './settings.js'
import { openLinksOutside } from './platform.js'
import { loadPeer, browserDeviceName } from './web/peer.js'
import { cloudTarget } from './web/cloud.js'
import { loadDevice, loadTokens, saveDevice } from './web/session.js'
import DeviceView from './web/DeviceView.jsx'
import SignInSurface from './web/SignInSurface.jsx'

// Before first paint: the persisted reduce-motion, density and privacy mode,
// so the page never flashes animated, mis-spaced or readable.
restoreBootPrefs(document, window.location)
openLinksOutside(document)

function WebRoot() {
  const [route, setRoute] = useState({ step: 'booting' })
  const [peer, setPeer] = useState(null)

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
      const tokens = loadTokens()
      if (!link || !saved || !tokens) {
        setRoute({ step: 'signin' })
        return
      }
      // The link has to be open before the device view can ask anything: the
      // peer module seats this browser again from the key it keeps, which is
      // cheap when the registry already knows it.
      try {
        await link.ensureDevice({
          api: cloudTarget({ search: window.location.search, env: window.__OPENREMOTE_CLOUD__ }).api,
          accessToken: tokens.access,
          name: browserDeviceName(),
        })
      } catch {
        if (!cancelled) setRoute({ step: 'signin' })
        return
      }
      if (!cancelled) setRoute({ step: 'device', machine: saved })
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const linked = async (machine) => {
    const link = peer ?? (await loadPeer())
    const tokens = loadTokens()
    if (!link || !tokens) {
      setRoute({ step: 'signin' })
      return
    }
    setPeer(link)
    // Picking a machine is what opens the link: the peer module makes sure this
    // browser has a seat in the account and a socket to the relay, and the
    // device view can only ask anything once that is done.
    try {
      await link.ensureDevice({
        api: cloudTarget({ search: window.location.search, env: window.__OPENREMOTE_CLOUD__ }).api,
        accessToken: tokens.access,
        name: browserDeviceName(),
      })
    } catch {
      setRoute({ step: 'signin' })
      return
    }
    saveDevice({ id: machine.id, name: machine.name })
    setRoute({ step: 'device', machine })
  }

  if (route.step === 'device') {
    return (
      <StrictMode>
        <DeviceView machine={route.machine} peer={peer} onLeave={() => setRoute({ step: 'signin' })} />
      </StrictMode>
    )
  }
  return (
    <StrictMode>
      {route.step === 'signin' && <SignInSurface onLinked={linked} />}
      <Splash />
    </StrictMode>
  )
}

createRoot(document.getElementById('root')).render(<WebRoot />)

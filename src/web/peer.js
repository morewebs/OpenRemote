// The encrypted link, as the web console sees it.
//
// The protocol lives in `src/peer/`: a WebAssembly build of the same Noise link
// the desktop speaks, the key pair this browser keeps, the registration that
// seats it in the account as one more device, and the relay socket. This module
// is the console's adapter to it, with one job: turn the shipped functions into
// the two things this page needs, in a shape that does not change if the peer
// is not in the build.
//
//   ensureDevice({ api, accessToken, name }) -> { id, name }
//     Registers this browser with the account, once, and returns the device id.
//
//   fetchFor(machineId) -> (url, init) -> Promise<Response>
//     The console's own fetch, carried over that link to the picked machine.
//     The URL starts with PEER_ORIGIN, which nothing resolves: the request path
//     is what crosses the link, so a gateway address a page supplied can never
//     choose where a sealed request goes.
//
// `src/web/session.js` already keeps the account tokens and the picked machine;
// the peer's own module keeps the device key, in its own store.

import wasmUrl from '../peer/openremote-webpeer.wasm?url'
// The link itself. A plain import rather than a glob: the peer is part of this
// build, and the glob wrapped it in Vite's preload helper, whose stylesheet
// wait never settled on the deployed site and left the page on its "checking"
// step for ever (measured 2026-10-31, the first production load).
import * as peerModule from '../peer/index.js'

function adapt(mod) {
  if (!mod || typeof mod.createWebPeer !== 'function') return null
  let session = null
  let opening = null
  // The registry base this page keeps is the API root (`.../v1/openremote`);
  // the peer module wants the gateway itself and appends the path.
  const gateway = (api) => String(api ?? '').replace(/\/v1\/openremote\/?$/, '').replace(/\/+$/, '')
  return {
    async ensureDevice({ api, accessToken, name }) {
      if (session) return { id: session.device.deviceId, name: session.device.name ?? name ?? 'Web console' }
      if (!opening) {
        opening = mod
          .createWebPeer({ api: gateway(api), token: accessToken, wasmUrl, name: name ?? 'Web console' })
          .then((opened) => {
            session = opened
            return opened
          })
          .finally(() => {
            opening = null
          })
      }
      const opened = await opening
      return { id: opened.device.deviceId, name: name ?? 'Web console' }
    },
    fetchFor(machineId) {
      // A refused promise rather than a throw: the device view builds its client
      // while it renders, and a synchronous throw there unmounts the whole app.
      if (!session) return () => Promise.reject(new Error('the link is not open yet'))
      return session.fetchFor(machineId)
    },
    close() {
      session?.close()
      session = null
    },
  }
}

let loaded = null
export async function loadPeer() {
  if (loaded !== null) return loaded
  // A page may install one before the module loads, which is how the link can
  // be tried against a stand-in without a build.
  const injected = typeof window !== 'undefined' ? window.__OPENREMOTE_PEER__ : null
  if (injected && typeof injected.fetchFor === 'function') {
    loaded = injected
    return loaded
  }
  try {
    loaded = adapt(peerModule)
  } catch {
    loaded = null
  }
  return loaded
}

/// Whether a loaded module can do both halves of the job.
export function peerReady(peer) {
  return Boolean(peer && typeof peer.fetchFor === 'function' && typeof peer.ensureDevice === 'function')
}

/// The device name this browser registers under: the platform and the browser
/// family, which is what a machine list needs to tell it from a computer.
export function browserDeviceName(nav = typeof navigator === 'undefined' ? null : navigator) {
  const ua = String(nav?.userAgent ?? '')
  const family = /edg\//i.test(ua)
    ? 'Edge'
    : /chrome|crios/i.test(ua)
      ? 'Chrome'
      : /firefox|fxios/i.test(ua)
        ? 'Firefox'
        : /safari/i.test(ua)
          ? 'Safari'
          : 'a browser'
  return `OpenRemote Web (${family})`
}

// The browser's transport: one authenticated fetch that answers the console's
// requests over the encrypted link to the machine the person picked.
//
// `src/api/daemon.js` asks its own `fetch` field for every call and for the
// live stream, so the console keeps every shape it already had and never
// learns that the wire is a relay. The address it dials is a placeholder on a
// reserved host: the link takes the request's path and headers, and this
// module never forwards the placeholder origin to the peer, which is why a
// page-supplied gateway address can change where the console signs in and
// never where a sealed request is sent.

/// The placeholder origin. Nothing resolves it, and nothing may.
export const PEER_ORIGIN = 'https://link.openremote.invalid'

let current = null

/// Install the transport for this page: `{ device, fetch }`, where `fetch` is
/// shaped like the global one. Called once, when a machine is picked.
export function setWebTransport(transport) {
  current = transport && typeof transport.fetch === 'function' ? transport : null
  return current
}

/// The installed transport, or null in the desktop and before a pick.
export function webTransport() {
  return current
}

/// The fetch a `DaemonApi` gets in the web build: the peer's own, or null so
/// the console shows its unreachable sentence rather than dialling a host that
/// does not exist.
export function webFetch() {
  return current?.fetch ?? null
}

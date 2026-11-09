// The web client's one routing rule: a machine id is not a `/devices/{id}`
// prefix. The desktop daemon forwards a peer request under that prefix because
// it is asking another daemon; the browser's peer fetch already carries the
// path to the machine, so the client must start at the root.

import { DaemonApi } from '../api/daemon.js'
import { PEER_ORIGIN } from './transport.js'

/// A daemon client whose requests cross the browser's encrypted link to one
/// machine. The peer fetch carries the path; there is no `/devices/{id}` in
/// front of it, and no local daemon behind it.
export function webApi(peer, machineId) {
  if (!peer || typeof peer.fetchFor !== 'function') {
    throw new Error('the web link is not open')
  }
  if (!machineId) {
    throw new Error('a machine is required to use the web link')
  }
  return new DaemonApi(PEER_ORIGIN, 'web-link', '', peer.fetchFor(machineId))
}

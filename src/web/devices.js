// The account's devices, in the words the web console's picker shows.
//
// The gateway answers `{ account, devices: [...] }`, and a device the browser
// can drive is the one whose `kind` is `machine`. Everything else is a place
// the account is signed in (a desktop, a phone, another browser), and offering
// one as a machine would be an offer the link cannot keep: `PEER_ROUTES` gives
// a non-machine device the Sync tier only.

/// One machine as the picker shows it.
export function machineOf(device) {
  if (!device || typeof device !== 'object') return null
  if (device.kind !== 'machine') return null
  if (typeof device.id !== 'string' || !device.id) return null
  const name = typeof device.name === 'string' && device.name.trim() ? device.name.trim() : 'An unnamed machine'
  return {
    id: device.id,
    name,
    online: device.online === true,
    platform: typeof device.platform === 'string' ? device.platform : '',
    version: typeof device.app_version === 'string' ? device.app_version : '',
  }
}

/// The machines of one device-list answer, the online ones first and each
/// group in name order, so the list does not reshuffle between polls.
export function machinesOf(payload) {
  const rows = Array.isArray(payload?.devices) ? payload.devices : []
  return rows
    .map(machineOf)
    .filter(Boolean)
    .sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name))
}

/// What the row says about reaching the machine: a machine that is asleep is
/// still one the account holds, and the sentence says which.
export function machineLine(machine) {
  if (!machine) return ''
  return machine.online ? 'Online' : 'Offline'
}

/// Why a row cannot be picked, or null when it can. A machine that is offline
/// is shown and explained rather than hidden: its chats are still there, and
/// the person may be about to wake it.
export function pickRefusal(machine) {
  if (!machine) return 'This device is not a machine.'
  if (!machine.online) return 'This machine is offline. Start its agent, then try again.'
  return null
}

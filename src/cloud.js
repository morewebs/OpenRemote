// Cloud mode's rules for the console, kept pure so they're tested without a
// daemon: which chats each mode lists, what can be done from here with a
// chat that runs on another device, and where a new Cloud chat can run.

const SIGNED_IN = new Set(['connecting', 'online', 'offline'])

/** This device is in Cloud (connected or not right now). */
export function signedIn(cloud) {
  return SIGNED_IN.has(cloud?.state)
}

/** The states the Cloud area answers with its sign-in screen. */
export function needsSignIn(cloud) {
  return !signedIn(cloud)
}

/**
 * Local lists everything that runs on this computer, private or synced;
 * Cloud lists every synced chat, wherever it runs.
 */
export function sessionsForMode(sessions, mode, thisDevice) {
  return (sessions ?? []).filter((s) =>
    mode === 'cloud' ? Boolean(s.executor) : !s.executor || s.executor === thisDevice,
  )
}

/**
 * What can be done from here with a chat:
 * - 'here': it runs on this computer;
 * - 'remote': it runs on a machine that can take requests now;
 * - 'offline': its device is offline (the copy stays readable);
 * - 'not-machine': it runs on a desktop that isn't a machine;
 * - 'signed-out': this computer isn't in Cloud, so it can't reach it;
 * - 'gone': its device left the account.
 */
export function chatAccess(session, devices, thisDevice, cloud) {
  if (!session?.executor || session.executor === thisDevice) return 'here'
  if (!signedIn(cloud)) return 'signed-out'
  const device = (devices ?? []).find((d) => d.id === session.executor)
  if (!device) return 'gone'
  if (!device.online) return 'offline'
  if (device.kind !== 'machine') return 'not-machine'
  return 'remote'
}

/** A device's name, for labels. */
export function deviceName(devices, id) {
  return (devices ?? []).find((d) => d.id === id)?.name ?? 'another device'
}

/**
 * Where a new Cloud chat can run: this computer first, then the account's
 * machines, online ones before offline ones.
 */
export function machinePickerItems(devices, thisDevice) {
  const machines = (devices ?? [])
    .filter((d) => d.id !== thisDevice && d.kind === 'machine')
    .sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name))
  return [
    { id: null, name: 'This computer', online: true, here: true },
    ...machines.map((d) => ({ id: d.id, name: d.name, online: Boolean(d.online), platform: d.platform })),
  ]
}

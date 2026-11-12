// Cloud mode's rules for the console, kept pure so they're tested without a
// daemon: which chats each mode lists, what can be done from here with a
// chat that runs on another device, and where a new Cloud chat can run.

import { list, text } from './shape.js'

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
 * Whether the daemon has answered about this machine's Cloud sign-in yet. A
 * null view (and a view with no state in it) is "not asked", not "signed out":
 * every start and every daemon restart spends a poll in that gap, and a
 * signed-in person who reads "Not signed in" for it is reading a guess. The
 * account row and the account view both ask this, so the two cannot disagree
 * about whether there is anything to say yet.
 */
export function cloudAnswered(cloud) {
  return typeof cloud?.state === 'string'
}

/**
 * Why a sign-in ended, in the daemon's own words, capitalised for a sentence
 * slot. The daemon knows the reason (a relay that answered 401, a file it
 * could not read); the door and the account row both show this, so neither
 * can state a different reason above or under it.
 */
export function daemonReason(cloud) {
  const text = cloud?.error
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : null
}

/**
 * The alert under the Cloud door, or nothing. Two sources feed it: the
 * daemon's reason, and the error a failed sign-in click came back with.
 * The click's error is usually the daemon's own sentence again, which the
 * body already says, and the same words twice read as two problems. The
 * comparison ignores case, since the body capitalises the daemon's words
 * for its slot and the click's error keeps them as sent.
 */
export function cloudAlert({ clickError = null, cloud = null, body = '', signingIn = false } = {}) {
  if (signingIn) return null
  const text = clickError ?? (cloud?.state !== 'relink' ? daemonReason(cloud) : null)
  if (!text) return null
  const same = String(text).trim().toLowerCase() === String(body ?? '').trim().toLowerCase()
  return same ? null : text
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
 * - 'unknown': the account is signed in but the device list has not answered,
 *   so this console cannot say whether the device is there;
 * - 'gone': the list answered, and the device is not in it.
 *
 * Unknown is not empty. An unanswered devices list is `[]`, and a signed-in
 * daemon's answered list always holds at least the daemon itself, so an empty
 * list is evidence of nothing: the daemon answers `devices: []` while its relay
 * link is still coming up, and the console holds `[]` until the first read
 * lands. Reading that as "gone" told a person their machine had left the
 * account every time the app started or the relay blinked. Only a list with an
 * answer in it can say a device is missing.
 */
export function chatAccess(session, devices, thisDevice, cloud) {
  if (!session?.executor || session.executor === thisDevice) return 'here'
  if (!signedIn(cloud)) return 'signed-out'
  const rows = list(devices)
  const device = rows.find((d) => d.id === session.executor)
  if (!device) return rows.length === 0 ? 'unknown' : 'gone'
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
 * machines, online ones before offline ones. A phone runs no chats, so on
 * one the list is the machines alone.
 */
export function machinePickerItems(devices, thisDevice, { phone = false } = {}) {
  // A device row is daemon data: the list can be an object where an array was
  // promised, and a name can be anything at all. `list` and `text` are the
  // client's one rule for that shape, so a malformed row sorts and reads
  // instead of throwing the view that asked for the picker.
  const machines = list(devices)
    .filter((d) => d.id !== thisDevice && d.kind === 'machine')
    .sort((a, b) => Number(b.online) - Number(a.online) || text(a.name).localeCompare(text(b.name)))
    .map((d) => ({ id: d.id, name: text(d.name), online: Boolean(d.online), platform: d.platform }))
  return phone ? machines : [{ id: null, name: 'This computer', online: true, here: true }, ...machines]
}

/** A phone drives the account's machines and is never one itself. */
export function isPhone(device) {
  return device?.platform === 'android' || device?.platform === 'ios'
}

/** What a device is, for its card: a phone, a machine, or a desktop. */
export function kindLabel(device) {
  if (isPhone(device)) return 'Phone'
  if (device?.kind === 'machine') return 'Machine'
  // A browser joins the account as its own kind; it is never a desktop.
  return device?.kind === 'web' ? 'Web console' : 'Desktop'
}

/** How this device names itself. */
export function selfLabel(device) {
  return isPhone(device) ? 'This phone' : 'This computer'
}

const PLATFORM_LABEL = { windows: 'Windows', macos: 'macOS', linux: 'Linux', android: 'Android', ios: 'iOS' }

/** A device's platform, in its owner's words. */
export function platformLabel(platform) {
  return PLATFORM_LABEL[platform] ?? platform
}

/**
 * Devices that joined the account since this computer last looked. With
 * zero-touch sign-in any login adds a device, so every new one is shown
 * once with a "Not you?" - the user's check on their own account. The
 * first look records a baseline instead of flagging everything.
 */
export function newDevices(devices, known, thisDevice) {
  if (!known) return []
  const seen = new Set(known)
  return (devices ?? []).filter((d) => d.id !== thisDevice && !seen.has(d.id))
}

/**
 * Whether this computer just became a machine from another device: it was
 * a desktop when last seen here, it's a machine now, and the change wasn't
 * made from this console.
 */
export function madeMachineElsewhere(kind, lastKind, setHere) {
  return kind === 'machine' && lastKind === 'desktop' && setHere !== 'machine'
}

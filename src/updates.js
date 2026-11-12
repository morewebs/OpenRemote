// Updates, as the console knows them: what the last look found, and a look
// once a day while the app runs (unless the user turned that off). The shell
// does the looking and the installing through Tauri's updater, which verifies
// every download against the public key the app was built with; nothing here
// runs on a phone or in a browser tab.
//
// The look is the updater plugin's own request for the release's latest.json,
// and nothing else: the console adds no id, no count and no header of its own.

import { hasTauri, isMobile } from './platform.js'

const AUTO_KEY = 'openremote-check-updates'
const LAST_KEY = 'openremote-last-update-check'

export const DAY = 24 * 3600 * 1000
// The first look waits for the app to come up, so it never competes with the
// daemon starting or the first paint.
export const FIRST_LOOK_AFTER = 30_000
// How often a running app asks itself whether a day has passed. Asking is
// free; looking happens at most once a day.
export const LOOK_EVERY = 3600 * 1000

let state = { checking: false, enabled: true, available: null, error: null, checkedAt: null, installing: false }
const listeners = new Set()

function set(change) {
  state = { ...state, ...change }
  for (const fn of listeners) fn()
}

export function updateState() {
  return state
}

export function subscribeUpdates(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

function readAuto(storage) {
  try {
    return storage.getItem(AUTO_KEY) !== 'off'
  } catch {
    return true
  }
}

export function autoCheck() {
  return typeof localStorage === 'undefined' ? true : readAuto(localStorage)
}

export function setAutoCheck(on) {
  try {
    localStorage.setItem(AUTO_KEY, on ? 'on' : 'off')
  } catch {
    /* storage unavailable: the default (on) stays */
  }
  set({})
}

/// Whether the automatic look is due. A last look in the future means the
/// clock was set back, and waiting for it to catch up could take years.
export function isDue({ auto, last, now }) {
  if (!auto) return false
  if (!Number.isFinite(last) || last <= 0 || last > now) return true
  return now - last >= DAY
}

async function shell(command) {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke(command)
}

/// Ask the shell whether a newer signed release exists. The shell answers
/// `{ enabled, available }`: a build without the signing key's public half
/// is not switched on and asks nothing of the network.
export async function checkForUpdates() {
  if (!hasTauri || isMobile || state.checking) return
  set({ checking: true, error: null })
  try {
    const found = await shell('update_check')
    set({ enabled: found?.enabled !== false, available: found?.available ?? null, checkedAt: Date.now() })
  } catch (e) {
    set({ error: e })
  } finally {
    set({ checking: false })
  }
}

/// Download, verify and install; the app restarts into the new version. The
/// shell stops the daemon first, so the installer can replace it.
export async function installUpdate() {
  set({ installing: true, error: null })
  try {
    await shell('update_install')
  } catch (e) {
    set({ installing: false, error: e })
  }
}

/// The once-a-day schedule. Every argument is there for the tests: the
/// console calls it bare. Returns a stop function.
export function startUpdateChecks({
  enabled = hasTauri && !isMobile,
  timers = globalThis,
  storage = typeof localStorage === 'undefined' ? null : localStorage,
  now = Date.now,
  check = checkForUpdates,
} = {}) {
  if (!enabled) return () => {}
  const due = () => {
    let last = 0
    try {
      last = Number(storage?.getItem(LAST_KEY) ?? 0)
    } catch {
      last = 0
    }
    if (!isDue({ auto: storage ? readAuto(storage) : true, last, now: now() })) return
    // The attempt is what is recorded, not the success: a release page that
    // is not answering is asked again tomorrow, not every hour.
    try {
      storage?.setItem(LAST_KEY, String(now()))
    } catch {
      /* the next start looks again */
    }
    try {
      const pending = check()
      if (pending && typeof pending.catch === 'function') pending.catch(() => {})
    } catch {
      /* the look reports its own failure in the state */
    }
  }
  const first = timers.setTimeout(due, FIRST_LOOK_AFTER)
  const every = timers.setInterval(due, LOOK_EVERY)
  return () => {
    timers.clearTimeout(first)
    timers.clearInterval(every)
  }
}

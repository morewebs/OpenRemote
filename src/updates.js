// Updates, as the console knows them: what the last look found, and a look
// every two hours while the app runs (unless the user turned that off). The
// shell does the looking and the installing through Tauri's updater, which
// verifies every download against the public key the app was built with;
// nothing here runs on a phone or in a browser tab.
//
// The look is the updater plugin's own request for the release's latest.json,
// and nothing else: the console adds no id, no count and no header of its own.

import { hasTauri, isMobile } from './platform.js'

const AUTO_KEY = 'openremote-check-updates'
const LAST_KEY = 'openremote-last-update-check'

const HOUR = 3600 * 1000
// The launch look waits for the app to come up, so it never competes with the
// daemon starting or the first paint. Every launch looks once, whatever the
// last look was: a restart is the user saying they want to see what is new.
export const FIRST_LOOK_AFTER = 30_000
// How long after a look the next one is due.
export const LOOK_EVERY = 2 * HOUR
// How often a running app asks whether a look is due. Asking reads one number
// from storage, so it is free. A look lands at the first ask after two hours
// have passed, so it can be up to this long after the two hours.
export const ASK_EVERY = 5 * 60 * 1000

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
  return now - last >= LOOK_EVERY
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

/// Which shell command the offer uses. Only a build that can install itself
/// installs (the shell says so with `can_install`); everything else gets the
/// release page. A reply without the flag is treated as link-only, so the
/// console never asks for an install the shell did not offer.
export function installCommand(available) {
  return available?.can_install === true ? 'update_install' : 'update_open_page'
}

/// Windows: the shell verifies the download and installs it, stopping the
/// daemon first, and the app restarts into the new version. Elsewhere the
/// shell opens the release page it read at the last check. The console does
/// not pass a URL either way: the shell opens only what it already holds, so
/// this code cannot send the browser anywhere else.
///
/// Resolves to `{ ok, error }`, so the confirmation can say what failed. The
/// state keeps the error as well.
export async function installUpdate() {
  const command = installCommand(state.available)
  if (command === 'update_open_page') {
    try {
      await shell(command)
      return { ok: true, error: null }
    } catch (error) {
      set({ error })
      return { ok: false, error }
    }
  }
  set({ installing: true, error: null })
  try {
    await shell(command)
    return { ok: true, error: null }
  } catch (error) {
    set({ installing: false, error })
    return { ok: false, error }
  }
}

/// What the top bar shows for an update. It shows only when the shell found a
/// newer release and updates are switched on here. It stays hidden while the
/// install runs, because the app is about to restart. It is red while chats
/// are running, since a restart stops them, and blue otherwise. Blue is the
/// console's cool accent: the palette has no separate blue.
export function updateBadge({ available = null, enabled = false, installing = false, runningChats = 0 } = {}) {
  const running = Math.max(0, Math.floor(Number(runningChats) || 0))
  return {
    show: available != null && Boolean(enabled) && !installing,
    tone: running > 0 ? 'red' : 'blue',
    label: 'Update Available',
    runningChats: running,
  }
}

/// The words of the confirmation. On Windows the install restarts the app, so
/// running chats stop. On Linux nothing restarts when the page opens; the
/// warning there says the chats stop when the user restarts after installing,
/// which is the only restart that can stop them.
export function confirmCopy({ available = null, runningChats = 0, canInstall = false } = {}) {
  const version = available?.version ?? ''
  const count = Math.max(0, Math.floor(Number(runningChats) || 0))
  const danger = count > 0
  const chats = count === 1 ? '1 running chat' : `${count} running chats`
  if (canInstall === true) {
    return {
      title: `OpenRemote ${version} is ready`,
      body: ['The app restarts to install it.'],
      warning: danger ? `${chats} will stop when the app restarts.` : null,
      primary: 'Install and restart',
      danger,
      secondary: 'Not now',
    }
  }
  return {
    title: `OpenRemote ${version} is on the release page`,
    body: ['This build cannot install updates itself. Open the release page to download the new version.'],
    warning: danger ? `${chats} will stop when you restart OpenRemote after installing.` : null,
    primary: 'Open release page',
    danger,
    secondary: 'Not now',
  }
}

/// The schedule: one look shortly after every launch, then one each time two
/// hours have passed since the last attempt. Every argument is there for the
/// tests: the console calls it bare. Returns a stop function.
export function startUpdateChecks({
  enabled = hasTauri && !isMobile,
  timers = globalThis,
  storage = typeof localStorage === 'undefined' ? null : localStorage,
  now = Date.now,
  check = checkForUpdates,
} = {}) {
  if (!enabled) return () => {}
  // `launch` is the look that follows the start of the app: it runs whatever
  // the last attempt was, and only the automatic-check switch can stop it.
  const look = (launch) => {
    const auto = storage ? readAuto(storage) : true
    let last = 0
    try {
      last = Number(storage?.getItem(LAST_KEY) ?? 0)
    } catch {
      last = 0
    }
    if (launch ? !auto : !isDue({ auto, last, now: now() })) return
    // The attempt is what is recorded, not the success: a release page that
    // is not answering is asked again in two hours, not at every ask.
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
  const first = timers.setTimeout(() => look(true), FIRST_LOOK_AFTER)
  const every = timers.setInterval(() => look(false), ASK_EVERY)
  return () => {
    timers.clearTimeout(first)
    timers.clearInterval(every)
  }
}

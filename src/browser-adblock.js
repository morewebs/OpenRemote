// Ad and tracker blocking, as Settings > Browser shows it. The daemon keeps
// all of it (`/browser/adblock`), on this computer only: whether blocking is
// on, the filter lists the user picked from its catalog, the lists' state and
// the sites the user allows ads on. Nothing is on and no list is picked until
// the user says so, and no list is downloaded before then; this module only
// reads and writes that state and words it.

/// The daemon's answer as the console reads it: anything it did not send
/// reads as off, nothing picked, nothing allowed.
export function readAdblock(raw) {
  const s = raw && typeof raw === 'object' ? raw : {}
  const lists = Array.isArray(s.lists)
    ? s.lists
        .filter((l) => l && typeof l.id === 'string' && typeof l.title === 'string')
        .map((l) => ({
          id: l.id,
          title: l.title,
          about: typeof l.about === 'string' ? l.about : '',
          licence: typeof l.licence === 'string' ? l.licence : '',
          home: typeof l.home === 'string' ? l.home : '',
          picked: l.picked === true,
          updated: typeof l.updated === 'number' ? l.updated : null,
        }))
    : []
  return {
    on: s.on === true,
    supported: s.supported === true,
    state: typeof s.state === 'string' ? s.state : 'off',
    error: typeof s.error === 'string' && s.error.trim() ? s.error.trim() : null,
    lists,
    sites: Array.isArray(s.sites) ? s.sites.filter((x) => typeof x === 'string') : [],
  }
}

/// The ids of the picked lists.
export function pickedIds(status) {
  return status.lists.filter((l) => l.picked).map((l) => l.id)
}

/// One list as a row names it: its title, then its licence beside it.
export function listLabel(list) {
  return list.licence ? `${list.title} (${list.licence})` : list.title
}

/// The sentence under the toggle: what blocking is doing right now.
export function adblockLine(status) {
  const picked = status.lists.filter((l) => l.picked)
  const names = picked.map((l) => l.title).join(' and ')
  switch (status.state) {
    case 'none':
      return 'On, but no filter list is picked, so nothing is blocked. Choose a list below.'
    case 'updating':
      return `Downloading ${names || 'the picked lists'}. Pages are not filtered until it arrives.`
    case 'error': {
      const said = status.error ? `: “${status.error.replace(/[.!?]+$/, '')}”` : ''
      return `The last download of ${names || 'the picked lists'} failed${said}. It is tried again within the hour.`
    }
    case 'on':
      return `Blocking with ${names}. Lists refresh weekly while this is on.`
    default:
      return picked.length
        ? `Off. ${names} ${picked.length > 1 ? 'are' : 'is'} picked; nothing is downloaded or blocked until this is on.`
        : 'Off. Nothing is downloaded or blocked until you turn it on and pick a filter list.'
  }
}

/// A site as the daemon keeps it: a host name, lower case, no scheme, path
/// or port. A pasted address gives its host; anything else is null.
export function siteOf(text) {
  const raw = String(text ?? '').trim()
  if (!raw) return null
  let host = raw
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(raw)) {
    try {
      host = new URL(raw).hostname
    } catch {
      return null
    }
  } else {
    host = raw.split(/[/?#]/)[0].replace(/:\d+$/, '')
  }
  host = host.toLowerCase().replace(/\.$/, '')
  if (!host || host.length > 253 || host.startsWith('.') || host.includes('..')) return null
  return /^[a-z0-9._\-[\]:]+$/.test(host) ? host : null
}

/// The state, fetched through `call(method, path, body)` (browser-store's
/// `browserCall`). A failure throws for the caller to word with errorCopy.
export async function loadAdblock(call) {
  return readAdblock(await call('GET', '/browser/adblock'))
}

/// Turn blocking on or off, and/or pick the lists (the whole set of ids).
export async function setAdblock(call, change) {
  const body = {}
  if (typeof change.on === 'boolean') body.on = change.on
  if (Array.isArray(change.lists)) body.lists = change.lists
  return readAdblock(await call('POST', '/browser/adblock', body))
}

/// Allow ads on a site, or block them there again.
export async function setAdblockSite(call, site, allow) {
  return readAdblock(await call('POST', '/browser/adblock/site', { site, allow }))
}

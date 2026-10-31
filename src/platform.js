// Where the console runs. The Android app has no window chrome and no
// tray, and a phone runs no chats of its own - it drives the user's
// machines - so on a phone the console is Cloud only. A browser can try
// the phone layout with `?mobile`.

export const hasTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

export const isMobile =
  typeof window !== 'undefined' &&
  ((hasTauri && /Android/i.test(navigator.userAgent)) ||
    new URLSearchParams(window.location.search).has('mobile'))

/**
 * A web address this console will hand to a browser, or nothing.
 *
 * A plugin catalog's `homepage` is text from the catalog and may name any
 * scheme: a `file:` link, or a protocol some program installed on this
 * computer registered. On a phone the shell passes the URL to the system
 * handler for that scheme, and a new tab in the desktop webview is not
 * meant for anything but a page either, so only http(s) leaves the app.
 */
export function webLink(url) {
  const text = String(url ?? '').trim()
  return /^https?:\/\/\S/i.test(text) ? text : null
}

/**
 * Opens a web page outside the app. In the Android app the shell does it
 * (Cloud's sign-in in an in-app browser tab, anything else in the phone's
 * browser); elsewhere a new tab. Anything that is not a web address is
 * refused here rather than at each caller, so no surface can hand a
 * `file:` link to the system; the refusal throws, so a caller that meant
 * to open a page reports it instead of pretending it did.
 *
 * Resolves true when the page was handed over, false when the browser
 * refused it (a blocked popup), which is the one case a caller can still
 * do something about - offering the link instead.
 */
export async function openExternal(url, { signIn = false } = {}) {
  const page = webLink(url)
  if (!page) throw new Error(`${JSON.stringify(String(url ?? ''))} is not a web address`)
  if (hasTauri) {
    const { invoke } = await import('@tauri-apps/api/core')
    if (isMobile) await invoke('open_url', { url: page, signIn })
    else await invoke('browser_open_external', { url: page })
    return true
  }
  return window.open(page, '_blank', 'noopener,noreferrer') !== null
}

/**
 * In the Android app a link that would open a new tab opens in the phone's
 * browser instead: the webview has no tabs, and following it would replace
 * the console.
 */
export function openLinksOutside(doc) {
  if (!isMobile || !hasTauri) return
  doc.addEventListener(
    'click',
    (e) => {
      const link = e.target instanceof Element ? e.target.closest('a[href]') : null
      if (!link || !/^https?:\/\//i.test(link.getAttribute('href') ?? '')) return
      e.preventDefault()
      openExternal(link.href).catch(() => {})
    },
    true,
  )
}

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
 * Opens a web page outside the app. In the Android app the shell does it
 * (Cloud's sign-in in an in-app browser tab, anything else in the phone's
 * browser); elsewhere a new tab.
 */
export async function openExternal(url, { signIn = false } = {}) {
  if (isMobile && hasTauri) {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('open_url', { url, signIn })
    return
  }
  window.open(url, '_blank', 'noopener,noreferrer')
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

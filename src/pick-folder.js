// The workspace picker: the native OS folder dialog under Tauri, nothing
// in browser mode (no dead buttons - the caller hides Browse there).
// Cloud machines get their own picker when remote check-in lands.

export const hasTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

/// Open the OS folder dialog; resolves to the chosen path or null.
export async function pickFolder() {
  if (!hasTauri) return null
  const { open } = await import('@tauri-apps/plugin-dialog')
  return open({ directory: true, multiple: false, title: 'Choose a workspace folder' })
}

export const canPickFolder = hasTauri

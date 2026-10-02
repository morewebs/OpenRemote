// New-task defaults: what the composer starts with. Console-side facts,
// persisted locally — they preselect, never constrain.

const KEY = 'openremote-defaults'

export function loadDefaults() {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}')
    return {
      harness: typeof raw.harness === 'string' ? raw.harness : null,
      model: typeof raw.model === 'string' ? raw.model : null,
      workspace: typeof raw.workspace === 'string' ? raw.workspace : null,
    }
  } catch {
    return { harness: null, model: null, workspace: null }
  }
}

export function saveDefaults(partial) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...loadDefaults(), ...partial }))
  } catch {
    /* storage unavailable */
  }
}

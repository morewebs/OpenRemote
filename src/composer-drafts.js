const PREFIX = 'openremote-draft:'

// Plain text only. Attachments retain their separate ownership and lifecycle.
export function createDraftStore(storage) {
  const values = new Map()
  const revisions = new Map()
  const listeners = new Set()
  const read = key => {
    if (!values.has(key)) {
      let text = ''
      try { text = storage?.getItem(PREFIX + key) ?? '' } catch { /* Memory still works. */ }
      values.set(key, typeof text === 'string' ? text : '')
    }
    return values.get(key)
  }
  const write = (key, value) => {
    const text = typeof value === 'function' ? value(read(key)) : value
    if (typeof text !== 'string' || text === read(key)) return
    values.set(key, text)
    revisions.set(key, (revisions.get(key) ?? 0) + 1)
    try {
      if (text) storage?.setItem(PREFIX + key, text)
      else storage?.removeItem(PREFIX + key)
    } catch { /* Quota or privacy settings must not stop typing. */ }
    for (const listener of listeners) listener()
  }
  return {
    read, write,
    snapshot: key => ({ text: read(key), revision: revisions.get(key) ?? 0 }),
    acknowledge: (key, sent) => {
      if (read(key) === sent.text && (revisions.get(key) ?? 0) === sent.revision) write(key, '')
    },
    subscribe: listener => { listeners.add(listener); return () => listeners.delete(listener) },
  }
}

let storage
try { storage = globalThis.sessionStorage } catch { /* Storage may be denied entirely. */ }
export const composerDrafts = createDraftStore(storage)

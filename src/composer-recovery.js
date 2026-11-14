import { addAttachment } from './attachments.js'

const drafts = new Map()
const listeners = new Set()

// A first prompt may fail after navigation. Its already-prepared reference
// content remains usable even if the original file is no longer available.
export function recoverDraft(key, text, items) {
  if (text) drafts.set(key, [drafts.get(key), text].filter(Boolean).join('\n\n'))
  for (const item of items) addAttachment(item, key)
  for (const fn of listeners) fn(key)
}

export function consumeRecoveredDraft(key) {
  const text = drafts.get(key) || ''
  drafts.delete(key)
  return text
}

export function onRecoveredDraft(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

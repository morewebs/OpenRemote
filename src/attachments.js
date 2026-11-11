// What waits to ride a composer's next message besides its text: pictures
// (pasted, dropped, a screenshot of a page) and elements picked in the
// built-in browser. Kept per composer in a plain module - a chat's id, or
// 'new' for the home screen's - so the browser can hand something to the
// composer the user is looking at without the two knowing each other.
//
// An item is one of:
//   { kind: 'image', name, mime, dataUrl }
//   { kind: 'element', label, details, url, title, dataUrl? }
// `details` is the element as text (role, name, text, selector, a short
// piece of its HTML), sent with the message; `dataUrl` its picture.

import { consoleError } from './error-copy.js'

const byComposer = new Map()
const listeners = new Set()
// The composer the user is in, as a claim only its own composer can end
// ({ key }), or null when none is on screen.
let focused = null
let next = 1

// What the browser says when it has a picture and no composer to put it in.
export const NO_COMPOSER = 'no chat is open to take the picture; open a chat, then try again'

function changed() {
  for (const fn of listeners) fn()
}

// The composer the user is in now: the browser's picks go there. Like
// composer-insert.js's claimInserter, the claim belongs to the composer that
// made it, and the returned release is what that composer's unmount calls:
// a picture added after a chat closed must not wait, unseen, in the closed
// chat's composer. A later claim is not this one's to clear.
export function setFocusedComposer(key) {
  const mine = { key: key || 'new' }
  focused = mine
  return function release() {
    if (focused === mine) focused = null
  }
}

// The focused composer's key ('new' for the home screen), or null when no
// composer holds the claim.
export function focusedComposer() {
  return focused?.key ?? null
}

export function attachments(key) {
  return byComposer.get(key || 'new') ?? []
}

export function addAttachment(item, key = focusedComposer()) {
  const id = `a${next++}`
  const list = attachments(key)
  byComposer.set(key || 'new', [...list, { ...item, id }])
  changed()
  return id
}

export function removeAttachment(key, id) {
  byComposer.set(key || 'new', attachments(key).filter((a) => a.id !== id))
  changed()
}

export function clearAttachments(key) {
  byComposer.delete(key || 'new')
  changed()
}

// A message's attachments moving with it (the home screen's composer
// becoming a new chat's).
export function moveAttachments(from, to) {
  const list = attachments(from)
  if (!list.length) return
  byComposer.delete(from || 'new')
  byComposer.set(to, [...attachments(to), ...list])
  changed()
}

// Add to the composer the user is in, and say which: `{ id, key }`. With no
// composer on screen it throws the console's own sentence (NO_COMPOSER), for
// errorCopy, rather than leaving the item somewhere nobody is looking.
export function addToFocused(item) {
  const key = focusedComposer()
  if (!key) throw consoleError(NO_COMPOSER)
  return { id: addAttachment(item, key), key }
}

export function subscribe(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

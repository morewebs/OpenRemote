// The New chat greeting. It is not one fixed line: a new chat starts on the
// next sentence, and an empty composer that sits still moves on too, so the
// page does not read as a screenshot left open.

export const GREETINGS = [
  'What are we working on?',
  'What should we pick up?',
  'Where do we start?',
  'What needs doing?',
  'What are we taking on?',
  'Ready when you are.',
]

const KEY = 'openremote-greeting'

export function greetingAt(index) {
  const i = Number(index)
  if (!Number.isInteger(i) || i < 0 || i >= GREETINGS.length) return GREETINGS[0]
  return GREETINGS[i]
}

export function nextGreetingIndex(current, length = GREETINGS.length) {
  const n = Number(current)
  if (!Number.isInteger(n) || n < 0 || n >= length) return 0
  return (n + 1) % length
}

/// The sentence a fresh New chat should open on: one past the last one this
/// browser showed, so two new chats in a row do not repeat.
export function greetingForNewChat(storage = globalThis.sessionStorage) {
  let last = -1
  try {
    const raw = storage?.getItem(KEY)
    last = raw == null || raw === '' ? -1 : Number(raw)
  } catch {
    last = -1
  }
  const index = nextGreetingIndex(Number.isInteger(last) ? last : -1)
  try {
    storage?.setItem(KEY, String(index))
  } catch {
    /* storage unavailable */
  }
  return index
}

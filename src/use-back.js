import { useEffect, useRef } from 'react'
import { createBackStack } from './back.js'
import { isMobile } from './platform.js'

// The one back stack, bound to the webview's history. Off a phone it is
// inert: the desktop has no back button to answer.
const stack =
  typeof window === 'undefined' ? null : createBackStack(window.history, { enabled: isMobile })
if (stack) window.addEventListener('popstate', (e) => stack.onPop(e.state))

/** Holds a back entry for `handler`; returns its release. */
export function pushBack(handler) {
  return stack ? stack.push(handler) : () => {}
}

/** While `open`, Android's back button calls `onClose`, newest open thing first. */
export function useBack(open, onClose) {
  const latest = useRef(onClose)
  latest.current = onClose
  useEffect(() => {
    if (!open) return undefined
    return pushBack(() => latest.current?.())
  }, [open])
}

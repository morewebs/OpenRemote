// The browser half of an armed destructive row: the arm ends the moment the
// row stops being the thing the user is looking at. Scrolling it out of view
// disarms it (an IntersectionObserver on the armed button itself), and so does
// losing focus (a tab, or a click somewhere else). Escape disarms it too:
// Escape or a click elsewhere keeps the row, which is what four panes were not
// doing: the armed control took the focus but answered no key, so
// the only way out of an armed row was to click something else. The listener is
// attached to the armed control here rather than in each pane, so the four cannot
// drift apart. The rule is armAfter in row-confirm.js; this only supplies the
// events.

import { useCallback, useEffect, useRef } from 'react'
import { armAfter, clearRow } from './row-confirm.js'

export function useArmedRow(armed, setArmed) {
  const watching = useRef(null)
  // The armed id, read at event time. The observer callback can land after a
  // re-render, and it has to compare against the arm that is live now, not the
  // one that was under it when the observer was built.
  const armedRef = useRef(armed)
  armedRef.current = armed
  // The Escape listener lives on the armed control, so the element it was
  // attached to is kept to detach it from.
  const keyed = useRef(null)

  // A callback ref on the armed button, so the observer follows the row that
  // is actually armed rather than the list. It is a useCallback because a
  // fresh function on every render makes React detach and re-attach the ref,
  // which would tear down and rebuild the observer on every re-render of an
  // armed row (a beat, a keystroke) for no change in what is watched.
  //
  // It also takes the keyboard when the armed control mounts. The first press
  // replaces the trash with "Remove it" in place, and the element that had the
  // focus is gone with the old button: without this, a keyboard user lands back
  // at the start of the document instead of on the control they just called
  // for. The sidebar's Unregister and the layout dialog's confirm do the same
  // with autoFocus; this is the hook's version of it.
  const buttonRef = useCallback(
    (el) => {
      watching.current?.disconnect?.()
      watching.current = null
      if (keyed.current) {
        keyed.current.el.removeEventListener('keydown', keyed.current.onKeyDown)
        keyed.current = null
      }
      if (!el) return
      el.focus?.({ preventScroll: true })
      // Escape keeps: the arm goes, nothing is removed, and the same press cannot
      // reach a dialog underneath it, because an armed row is a question the
      // person is being asked.
      const onKeyDown = (event) => {
        if (event.key !== 'Escape') return
        event.preventDefault()
        event.stopPropagation()
        setArmed(() => clearRow())
      }
      el.addEventListener('keydown', onKeyDown)
      keyed.current = { el, onKeyDown }
      if (typeof IntersectionObserver === 'undefined') return
      const observer = new IntersectionObserver((entries) => {
        if (!entries.some((entry) => !entry.isIntersecting)) return
        setArmed((current) => armAfter(current, armedRef.current, { visible: false }))
      })
      observer.observe(el)
      watching.current = observer
    },
    [setArmed],
  )

  useEffect(
    () => () => {
      watching.current?.disconnect?.()
      watching.current = null
      if (keyed.current) {
        keyed.current.el.removeEventListener('keydown', keyed.current.onKeyDown)
        keyed.current = null
      }
    },
    [],
  )

  // The id is read at event time here too: a blur whose handler was captured
  // for a row armed a beat ago must not disarm the row that is armed now.
  const onBlur = () => setArmed((current) => armAfter(current, armedRef.current, { focused: false }))

  return { buttonRef, onBlur }
}

// "Screenshot a region" (ported from the browser branch's BrowserRegion,
// 0d3211a): over the page's picture - the page steps aside while this is
// open, because `.bw-region` is one of browser-occlusion.js's floating
// surfaces - the user drags a rectangle, and letting go sends that part of
// the page to the chat. Esc cancels. The rectangle is reported in this
// box's own pixels.

import { useRef, useState } from 'react'
import { tooSmall } from './browser-attach.js'
import { useDialog } from './dialog-focus.js'

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v))
}

export default function BrowserRegion({ box, onPick, onCancel }) {
  const panelRef = useRef(null)
  const start = useRef(null)
  const [rect, setRect] = useState(null)

  // The dialog family's plumbing: the keyboard comes here, the rest of the
  // app waits, and Esc (through the shared layer stack) cancels.
  useDialog(panelRef, onCancel)

  const at = (e) => {
    const r = panelRef.current.getBoundingClientRect()
    return { x: clamp(e.clientX - r.left, 0, r.width), y: clamp(e.clientY - r.top, 0, r.height) }
  }

  const span = (a, b) => ({
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  })

  return (
    <div
      ref={panelRef}
      className="bw-region"
      role="dialog"
      aria-label="Select a part of the page"
      tabIndex={-1}
      style={{ left: box.left, top: box.top, width: box.width, height: box.height }}
      onPointerDown={(e) => {
        if (e.button !== 0) return
        e.currentTarget.setPointerCapture(e.pointerId)
        start.current = at(e)
        setRect(null)
      }}
      onPointerMove={(e) => {
        if (start.current) setRect(span(start.current, at(e)))
      }}
      onPointerUp={(e) => {
        if (!start.current) return
        const picked = span(start.current, at(e))
        start.current = null
        // A click, not a drag: keep waiting for one.
        if (tooSmall(picked)) {
          setRect(null)
          return
        }
        onPick(picked)
      }}
    >
      {rect ? (
        <div className="bw-region-rect" style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }} />
      ) : (
        <div className="bw-region-shade" />
      )}
      <div className="bw-region-hint">Drag over the part of the page to send to the chat. Esc cancels.</div>
    </div>
  )
}

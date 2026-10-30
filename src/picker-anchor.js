// Where a dropdown opens. The console anchors a picker on the pointer, which
// is right for a mouse and impossible for a keyboard: a click that Enter or
// Space sends carries no coordinates, both read 0, and the menu lands on the
// window's top-left corner instead of under the control that opened it. The
// engine asks this module instead, so the rule lives in one place rather than
// in each caller's event handler.

// A screen point a mouse actually produced. 0,0 is the signature of a
// synthesized click: no control sits at the very corner of the window under a
// real cursor, so the pair is safe to read as "no point".
export function isRealPoint(anchor) {
  if (!anchor) return false
  const { left, top } = anchor
  return Number.isFinite(left) && Number.isFinite(top) && (left !== 0 || top !== 0)
}

// The spot under a trigger: its left edge, just below it. A rect with no size
// is no trigger (a hidden button, a detached node), so it answers null and the
// caller falls back.
// The model list hangs off the composer, not the click. Its left edge is the
// box's own left padding, and its top is just under the box, so the menu
// lines up with the field instead of the pointer.
export function spotAtBox(rect, { padLeft = 0, gap = 0 } = {}) {
  if (!rect || !(rect.width > 0) || !(rect.height > 0)) return null
  return { left: Math.round(rect.left + padLeft), top: Math.round(rect.bottom + gap) }
}

export function spotBelow(rect) {
  if (!rect || !(rect.width > 0) || !(rect.height > 0)) return null
  return { left: Math.round(rect.left), top: Math.round(rect.bottom + 6) }
}

// The anchor a picker should use: the pointer when there was one, the trigger's
// own place when there was not, and the window's corner only when neither is
// known (a menu opened from code, with nothing focused to hang off).
export function resolvePickerSpot(anchor, triggerRect) {
  if (isRealPoint(anchor)) return anchor
  return spotBelow(triggerRect) ?? { left: 8, top: 8 }
}

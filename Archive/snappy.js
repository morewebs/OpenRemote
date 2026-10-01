// Wheel scrolling that answers within a frame and settles fast.
//
// Chromium's native wheel animation glides for ~160ms after the last
// tick — on a 144hz display that trail reads as input latency, while
// instant jumps (disable-smooth-scrolling) read as a slideshow. This
// handler moves each tick through a short exponential settle (~90ms):
// the first hop lands on the very next frame, positions interpolate
// at display rate, and the scroll stops when the hand stops.
//
// Trackpads and free-spinning wheels stream events faster than
// WHEEL_GAP apart; those stay on the native path, which is already 1:1.

const SELECTOR = '.cv-scroll, .sb-list, .panel, .devices'
const TAU = 12 // ms; per-tick settle ≈ TAU * ln(distance) — ~55ms for
// a standard 114px notch, roughly 2x tighter than Chromium's glide
const WHEEL_GAP = 24 // ms between events; denser streams are native

const wired = new WeakSet()

function attach(el) {
  if (wired.has(el)) return
  wired.add(el)
  let target = null
  let raf = 0
  let lastFrame = 0
  let lastAt = 0

  const step = (t) => {
    if (target == null) {
      raf = 0
      return
    }
    const dt = lastFrame ? Math.min(64, t - lastFrame) : 16
    lastFrame = t
    const d = target - el.scrollTop
    if (Math.abs(d) < 1) {
      el.scrollTop = target
      target = null
      raf = 0
      return
    }
    el.scrollTop += d * (1 - Math.exp(-dt / TAU))
    raf = requestAnimationFrame(step)
  }

  el.addEventListener(
    'wheel',
    (e) => {
      if (window.__snappyOff) return // A/B kill switch for measuring
      const dense = e.timeStamp - lastAt < WHEEL_GAP
      lastAt = e.timeStamp
      if (e.ctrlKey || e.deltaX !== 0 || e.deltaMode !== 0 || Math.abs(e.deltaY) < 10 || dense) return
      const max = el.scrollHeight - el.clientHeight
      if (max <= 1) return
      e.preventDefault()
      target = Math.max(0, Math.min(max, el.scrollTop + e.deltaY))
      if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
        el.scrollTop = target
        target = null
        return
      }
      if (!raf) {
        lastFrame = 0
        raf = requestAnimationFrame(step)
      }
    },
    { passive: false },
  )
}

function sweep(root) {
  if (root.nodeType !== 1) return
  if (root.matches?.(SELECTOR)) attach(root)
  root.querySelectorAll?.(SELECTOR).forEach(attach)
}

function start() {
  sweep(document.body)
  // React remounts scroll containers on view switches; wire new ones
  // as they appear. Listeners die with their elements, WeakSet dedupes.
  new MutationObserver((muts) => {
    for (const m of muts) for (const n of m.addedNodes) sweep(n)
  }).observe(document.body, { childList: true, subtree: true })
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', start)
} else {
  start()
}

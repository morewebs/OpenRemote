// A web page in the browser is a native view drawn above the console, so
// any of the console's own menus or dialogs that overlaps it would hide
// behind it. While one does, the browser shows a picture of the page in
// its place. This decides when. Pure: tested without a DOM.

// The console's floating surfaces: menus, the command palette, dialogs,
// the narrow layout's sidebar backdrop, and the region picker (which draws
// over the page's picture).
export const FLOATING = [
  '.nc-menu',
  '.cm-menu',
  '.dv-modal-backdrop',
  '.kb-backdrop',
  '.si-backdrop',
  '.ab-backdrop',
  '.sb-backdrop',
  '.vp-backdrop',
  '.bw-region',
].join(',')

export function overlaps(a, b) {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

// Whether any of these rectangles covers part of the page's.
export function covered(page, rects) {
  if (!page || page.width <= 0 || page.height <= 0) return false
  return rects.some((r) => r.width > 0 && r.height > 0 && overlaps(page, r))
}

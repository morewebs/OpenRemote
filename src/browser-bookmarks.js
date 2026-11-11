// Bookmarks' small rules: the two folders Chrome has (the bar and Other
// bookmarks), their order, where a new bookmark lands, what a bookmark with
// no name is drawn as, and what pressing the star on the current address
// means. A bookmark is { id, url, title, folder, position }; positions are
// numbers, so a move writes only the bookmark that moved. Pure: tested
// without a DOM, and no account anywhere - bookmarks stay on this device.

export const FOLDERS = { bar: 'Bookmarks bar', other: 'Other bookmarks' }

export function folderOf(mark) {
  return mark?.folder === 'other' ? 'other' : 'bar'
}

export function sortBookmarks(marks) {
  return [...(marks ?? [])].sort(
    (a, b) =>
      (folderOf(a) === folderOf(b) ? 0 : folderOf(a) === 'bar' ? -1 : 1) ||
      (a.position ?? 0) - (b.position ?? 0) ||
      String(a.id).localeCompare(String(b.id)),
  )
}

// One folder's bookmarks, in order.
export function inFolder(marks, folder) {
  return sortBookmarks((marks ?? []).filter((m) => folderOf(m) === folder))
}

export function bookmarkFor(marks, url) {
  if (!url) return null
  return (marks ?? []).find((m) => m.url === url) ?? null
}

export function isBookmarked(marks, url) {
  return bookmarkFor(marks, url) !== null
}

// The name a bookmark is drawn by: the one it was given, or the address
// without its scheme and trailing slash. A bookmark that was never named is
// still a bookmark, never an empty chip on the bar.
export function labelFor(mark) {
  const title = String(mark?.title ?? '').trim()
  if (title) return title
  return String(mark?.url ?? '')
    .replace(/^https?:\/\//i, '')
    .replace(/\/$/, '')
}

// The position that puts a bookmark at `index` in a folder's list (the list
// without the bookmark being moved): between its new neighbours.
export function positionAt(list, index) {
  const before = list[index - 1]?.position
  const after = list[index]?.position
  if (before == null && after == null) return 1
  if (before == null) return after - 1
  if (after == null) return before + 1
  return (before + after) / 2
}

// A bookmark moved one step up or down its folder: its new position, or null
// when it is already at that end.
export function stepPosition(marks, mark, step) {
  const list = inFolder(marks, folderOf(mark))
  const at = list.findIndex((m) => m.id === mark.id)
  const to = at + step
  if (at === -1 || to < 0 || to >= list.length) return null
  const rest = list.filter((m) => m.id !== mark.id)
  return positionAt(rest, to)
}

// The bookmark the star on this address should open, or the one it should
// add. `existing` means the address is already kept: the star then edits that
// bookmark instead of making a second copy, which is what Chrome does and
// what keeps one address one row on the bar.
export function starFor(marks, { url, title = '', folder = 'bar', id = '' } = {}) {
  if (!url) return null
  const existing = bookmarkFor(marks, url)
  if (existing) return { existing: true, mark: existing }
  return {
    existing: false,
    mark: {
      id,
      url,
      title: String(title ?? '').trim(),
      folder: folderOf({ folder }),
      position: positionAt(inFolder(marks, folderOf({ folder })), Number.MAX_SAFE_INTEGER),
    },
  }
}

// The list with `mark` added, or replacing the bookmark that shares its id
// (an edit keeps its place on the bar).
export function withBookmark(marks, mark) {
  const rest = (marks ?? []).filter((m) => m.id !== mark.id)
  return [...rest, mark]
}

export function withoutBookmark(marks, id) {
  return (marks ?? []).filter((m) => m.id !== id)
}

// ---- the bookmarks manager ----

// A folder's bookmarks that match what was typed in the manager's search: by
// name or by address, ignoring case. An empty search is the whole folder.
export function matching(marks, folder, query = '') {
  const needle = String(query ?? '').trim().toLowerCase()
  const list = inFolder(marks, folder)
  if (!needle) return list
  return list.filter((m) => String(m.title ?? '').toLowerCase().includes(needle) || String(m.url ?? '').toLowerCase().includes(needle))
}

// Where a bookmark lands when it is dropped at `index` of `folder`, counted
// as the folder shows now with the moving bookmark still in it: its folder
// and its new position. Only the moved bookmark is written.
export function placeAt(marks, mark, folder, index) {
  const to = folder === 'other' ? 'other' : 'bar'
  const full = inFolder(marks, to)
  const from = full.findIndex((m) => m.id === mark.id)
  const at = from !== -1 && from < index ? index - 1 : index
  const rest = full.filter((m) => m.id !== mark.id)
  return { folder: to, position: positionAt(rest, Math.max(0, Math.min(at, rest.length))) }
}

// The other folder, for "Move to ...".
export function otherFolder(mark) {
  return folderOf(mark) === 'other' ? 'bar' : 'other'
}

// A rename in the manager ends once. Enter saves and Esc cancels, and each
// ends the rename, which takes the focused field off the page; the engine
// may then fire a blur on it, and that blur is not a second ending (it
// would save a rename the person cancelled, or write a saved one twice). A
// blur on its own - a click elsewhere - saves, as it does in Chrome.
export function renameEnding(onRename) {
  let settled = false
  const end = (mark, title) => {
    if (settled) return false
    settled = true
    onRename(mark, title)
    return true
  }
  return {
    enter: (mark, title) => end(mark, title),
    escape: (mark) => end(mark, null),
    blur: (mark, title) => end(mark, title),
  }
}

// The one composer an attachment lands in: every composer (a new chat,
// a running chat) claims the bus when it mounts and when it is focused,
// and the browser pane's "attach to chat" inserts through whoever claimed
// last - where the user is typing. Plain module, no DOM: tested.

let current = null
const composers = new Map()
const generations = new Map()

// Claim the bus for an inserter, and hand back the release that ends the
// claim. The claim belongs to the composer that made it: releasing it is what
// a composer's unmount does, so a claim cannot outlive the surface it types
// into and the browser pane never reports a delivery that went nowhere.
// `key` names the composer (a chat's id, or 'new'), so the pane can say
// which chat it shared with.
export function claimInserter(fn, key = null, owner = null) {
  const mine = { fn, key, owner: owner || {} }
  current = mine
  if (key) composers.set(key, mine)
  return function release() {
    // A later claim is not this one's to clear: the composer that took the bus
    // after this one still owns it, and only its own unmount ends that.
    if (current === mine) current = null
    if (key && composers.get(key) === mine) composers.delete(key)
  }
}

// Insert text into the claiming composer. False when no live composer holds
// the bus (no chat open, or the one that claimed has gone away), so the
// caller can say so instead of claiming it was delivered.
export function insertToComposer(text) {
  if (!current) return false
  current.fn(text)
  return true
}

// The composer that holds the bus, by its key, or null.
export function inserterKey() {
  return current?.key ?? null
}

// Test-only: forget the claim.
export function resetInserter() {
  current = null
  composers.clear()
  generations.clear()
}

// Capture an ownership claim before async work starts. A later focus or a
// newly mounted composer with the same chat id cannot receive its result.
export function composerTarget(key) {
  const mine = composers.get(key)
  if (!mine) return null
  const generation = generations.get(key) || 0
  const isLive = () => composers.get(key)?.owner === mine.owner && (generations.get(key) || 0) === generation
  return { key, isLive, insert: text => {
    if (!isLive()) return false
    mine.fn(text)
    return true
  } }
}

export function advanceComposerDraft(key) {
  generations.set(key, (generations.get(key) || 0) + 1)
}

// Android's back button goes back in the webview's history, and leaves the
// app when there is none. So on a phone, whatever back should undo - an
// open modal or menu, the sidebar drawer, the last view - holds one history
// entry, and back runs the newest one's handler. Pure over a `history`-like
// object, so it's tested without a browser.
//
// Two rules keep a press honest. Closing something some other way (its X, a
// choice, Esc) takes its entry with it while that entry is still the newest,
// so it never lingers as a press that visibly does nothing. And every pop
// this module causes itself is counted: the popstate that answers it must
// not be mistaken for the user's press, and must not close what is below.

export function createBackStack(history, { enabled = true } = {}) {
  // depth -> handler, for the entries pushed above the page's own.
  const handlers = new Map()
  // Pops this module asked for and has not seen answered yet. A real
  // webview delivers popstate asynchronously, so a second press arriving
  // mid-unwind must not be read as a second unwind.
  let pendingPops = 0
  // history.back() finishes asynchronously. A new overlay must wait for
  // that pop before it pushes, or the old pop consumes its fresh entry.
  let waiting = []
  const depthOf = (state) => (state && typeof state.backDepth === 'number' ? state.backDepth : 0)

  function register(entry) {
    entry.depth = depthOf(history.state) + 1
    history.pushState({ ...(history.state ?? {}), backDepth: entry.depth }, '')
    handlers.set(entry.depth, entry.handler)
  }

  function popEntry() {
    pendingPops += 1
    history.back()
  }

  /**
   * Holds a history entry while something back should close is open.
   * Returns the release to call when it closes some other way (its X, a
   * choice, Esc): the entry goes with it, so back lands on the thing the
   * user was looking at rather than on a gone one.
   */
  function push(handler) {
    if (!enabled) return () => {}
    const entry = { handler, depth: null }
    if (pendingPops > 0) waiting.push(entry)
    else register(entry)
    return () => {
      if (entry.depth == null) {
        waiting = waiting.filter((item) => item !== entry)
        return
      }
      const depth = entry.depth
      if (handlers.get(depth) !== handler) return
      handlers.delete(depth)
      // Only the newest entry can be popped safely; one with something
      // still open above it is left for the pop above to take with it.
      if (depthOf(history.state) === depth) popEntry()
    }
  }

  /** The popstate listener: run what was popped, skip spent entries. */
  function onPop(state) {
    const landed = depthOf(state)
    if (pendingPops > 0) {
      // One of ours: the entries above the landing point are spent by
      // definition, so they are dropped without closing anything.
      pendingPops -= 1
      for (const depth of [...handlers.keys()]) {
        if (depth > landed) handlers.delete(depth)
      }
      if (pendingPops === 0) {
        const ready = waiting
        waiting = []
        for (const entry of ready) register(entry)
      }
      return
    }
    let ran = false
    for (const depth of [...handlers.keys()].sort((a, b) => b - a)) {
      if (depth <= landed) continue
      const handler = handlers.get(depth)
      handlers.delete(depth)
      if (handler && !ran) {
        ran = true
        handler()
      }
    }
    // Only spent entries were popped: one back press is one visible step,
    // so walk down to the entry the user is actually on.
    if (!ran && landed > 0) popEntry()
  }

  return { push, onPop }
}

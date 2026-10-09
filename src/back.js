// Android's back button goes back in the webview's history, and leaves the
// app when there is none. So on a phone, whatever back should undo - an
// open modal or menu, the sidebar drawer, the last view - holds one history
// entry, and back runs the newest one's handler. Pure over a `history`-like
// object, so it's tested without a browser.

export function createBackStack(history, { enabled = true } = {}) {
  // depth -> handler, for the entries pushed above the page's own.
  const handlers = new Map()
  const depthOf = (state) => (state && typeof state.backDepth === 'number' ? state.backDepth : 0)

  /**
   * Holds a history entry while something back should close is open.
   * Returns the release to call when it closes some other way (its X, a
   * choice, Esc): the entry stays behind as a spent one, and back steps
   * over it without a stop.
   */
  function push(handler) {
    if (!enabled) return () => {}
    const depth = depthOf(history.state) + 1
    history.pushState({ ...(history.state ?? {}), backDepth: depth }, '')
    handlers.set(depth, handler)
    return () => {
      if (handlers.get(depth) === handler) handlers.set(depth, null)
    }
  }

  /** The popstate listener: run what was popped, skip spent entries. */
  function onPop(state) {
    const landed = depthOf(state)
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
    // Only spent entries were popped: one back press is one visible step.
    if (!ran && landed > 0) history.back()
  }

  return { push, onPop }
}

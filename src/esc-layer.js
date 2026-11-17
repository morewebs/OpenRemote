// Esc closes one layer, the top one. Every overlay that closes on Esc holds a
// layer while it is open, and the newest one holds the top: a dialog opened
// over another (a delete confirm over Settings, the endpoint manager over a
// rule) takes the key and the layer underneath stays open. Without it each
// overlay ran its own window listener for the same press, so one Esc closed
// the whole stack - and in "Manage endpoints" it took a half-written rule
// with it. The stack is plain and tested; the hook binds it to a component.

import { useEffect, useRef } from 'react'

export function createEscLayers() {
  let layers = []
  return {
    // A layer for as long as the caller is open, and a way to ask whether it
    // is still the top one.
    claim() {
      const mine = {}
      layers = [...layers, mine]
      return {
        isTop: () => layers[layers.length - 1] === mine,
        // An older layer's close never lifts a newer one's claim.
        release: () => {
          layers = layers.filter((layer) => layer !== mine)
        },
      }
    },
    depth: () => layers.length,
    blocksShortcut: combo => combo === 'Esc' && layers.length > 0,
  }
}

const layers = createEscLayers()
export const escapeLayerOwnsKey = combo => layers.blocksShortcut(combo)

/**
 * While `open`, Esc closes this layer - only when it is the top one, so the
 * press never falls through to whatever this was opened over. The key is
 * consumed when it is answered: prevented, and stopped from reaching the
 * layers below.
 */
export function useEscLayer(open, onClose) {
  const latest = useRef(onClose)
  latest.current = onClose
  useEffect(() => {
    if (!open) return undefined
    const claim = layers.claim()
    const onKey = (e) => {
      if (e.key !== 'Escape') return
      // Something closer to the keyboard already took this Esc: a picker
      // inside the layer, a shortcut being recorded.
      if (e.defaultPrevented) return
      if (!claim.isTop()) return
      e.preventDefault()
      e.stopPropagation()
      latest.current?.()
    }
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('keydown', onKey)
      claim.release()
    }
  }, [open])
}

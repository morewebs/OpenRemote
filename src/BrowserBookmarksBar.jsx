// The bookmarks bar under the tab strip (Chrome's): the bar folder's
// bookmarks left to right, the ones that do not fit behind a » menu, and
// Other bookmarks at the right end. A click opens the page here, a middle
// click or Ctrl+click in a new tab, right-click has the list's menu.

import { useLayoutEffect, useRef, useState } from 'react'
import { CaretDoubleRight, FolderSimple, Globe } from '@phosphor-icons/react'
import { inFolder, labelFor } from './browser-bookmarks.js'

export default function BrowserBookmarksBar({ marks, onOpen, onMenu, onFolderMenu, precapture }) {
  const bar = inFolder(marks, 'bar')
  const other = inFolder(marks, 'other')
  const listRef = useRef(null)
  // How many of the bar's bookmarks fit; the rest are in the » menu.
  const [fit, setFit] = useState(bar.length)

  useLayoutEffect(() => {
    const el = listRef.current
    if (!el) return undefined
    const measure = () => {
      const { right } = el.getBoundingClientRect()
      let n = 0
      for (const child of el.children) {
        if (child.getBoundingClientRect().right > right) break
        n += 1
      }
      setFit(n)
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return undefined
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [marks])

  return (
    // Its menus cover the page: a picture is taken as the pointer arrives.
    <div className="bw-marks" aria-label="Bookmarks bar" onPointerEnter={precapture}>
      <div className="bw-marks-list" ref={listRef}>
        {bar.map((m, i) => (
          <button
            key={m.id}
            type="button"
            className="bw-mark"
            style={i >= fit ? { visibility: 'hidden' } : undefined}
            tabIndex={i >= fit ? -1 : 0}
            title={`${labelFor(m)}\n${m.url}`}
            onMouseDown={(e) => {
              // A middle click opens a tab, never the page's autoscroll.
              if (e.button === 1) e.preventDefault()
            }}
            onClick={(e) => onOpen(m.url, { newTab: e.ctrlKey || e.metaKey })}
            onAuxClick={(e) => {
              if (e.button === 1) onOpen(m.url, { newTab: true })
            }}
            onContextMenu={(e) => {
              e.preventDefault()
              e.stopPropagation()
              onMenu(m, e)
            }}
          >
            <Globe size={14} className="bw-tab-icon" />
            <span className="bw-mark-title">{labelFor(m)}</span>
          </button>
        ))}
        {!bar.length && <span className="bw-marks-empty">Bookmark a page with the star to keep it here.</span>}
      </div>
      {fit < bar.length && (
        <button
          type="button"
          className="bw-btn bw-marks-more"
          title="More bookmarks"
          onClick={(e) => onFolderMenu(bar.slice(fit), e)}
        >
          <CaretDoubleRight size={14} />
        </button>
      )}
      {other.length > 0 && (
        <button type="button" className="bw-mark bw-mark-folder" onClick={(e) => onFolderMenu(other, e)}>
          <FolderSimple size={14} className="bw-tab-icon" />
          <span className="bw-mark-title">Other bookmarks</span>
        </button>
      )}
    </div>
  )
}

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { Check, ClockCounterClockwise, MagnifyingGlass } from '@phosphor-icons/react'
import { isMobile } from './platform.js'
import { useBack } from './use-back.js'
import { fitPicker, resolvePickerSpot } from './picker-anchor.js'
import './pickermenu.css'

/**
 * The cozy dropdown engine shared by the model / harness / device pickers.
 * Opens at the cursor, searches, groups (optional), keyboard-navigates,
 * clamps to the viewport, scrolls internally.
 *
 * A row may carry the catalog's own two extra facts, both rendered as the
 * harness sent them: `detail`, a muted second line under the name (a model's
 * cost tier), and `note`, the row's hover sentence.
 */
export default function PickerMenu({
  label,
  searchPlaceholder,
  items,
  groups, // [{ label, items }] or null for a flat list
  selectedId,
  onChoose,
  onClose,
  anchor, // { left, top }, from the pointer that opened the menu when there was one
  getAnchor, // a live phone anchor when the keyboard moves the composer
  portal = false, // escape the phone sheet, except when a dialog owns the menu
  renderIcon,
  renderTrailing,
  renderSubline, // a muted second line inside the item (efforts, paths)
  wide, // room for sublines without cramped trailing text
  footer, // a pinned action row under the list; renders in the empty state too
  note, // one line under the search field, for a list the caller knows is stale
  hideSearch = false,
  leaving = false,
  place = 'below',
}) {
  const [query, setQuery] = useState('')
  const [cursor, setCursor] = useState(0)
  const ref = useRef(null)
  const searchRef = useRef(null)
  // The element that opened this menu, kept for two uses: its rect places a menu opened
  // from the keyboard, and it takes the keyboard back when the menu closes. Both are
  // knowable only during the first render, while the trigger still holds focus, which
  // is why the ref is filled from the state initialiser rather than an effect.
  const triggerRef = useRef(null)
  // The spot is decided once, during the first render, while the trigger that
  // opened the menu still holds focus: a keyboard activation reports no point,
  // and that is the only moment the trigger is knowable. See picker-anchor.js.
  const [spot] = useState(() => {
    const active = typeof document === 'undefined' ? null : document.activeElement
    triggerRef.current = active && active !== document.body ? active : null
    return resolvePickerSpot(anchor, triggerRef.current?.getBoundingClientRect?.() ?? null)
  })

  const filtered = (() => {
    const q = query.trim().toLowerCase()
    if (!q) return items
    // A caller's row can carry its label under another field, or none yet
    // while the list loads: a nameless row is an empty match, never a throw
    // on the first keystroke.
    return items.filter((i) => String(i.name ?? '').toLowerCase().includes(q))
  })()

  useBack(true, onClose)

  useEffect(() => {
    // On a phone the keyboard would cover the list it's searching; the
    // search field is a tap away.
    if (!hideSearch && !isMobile) searchRef.current?.focus()
    else ref.current?.focus()
    // The trigger is what held focus when this menu opened (see the comment above):
    // without this, closing the menu left focus on the document body, so a person who
    // chose with Enter had lost their place in the page.
    const returnTo = triggerRef.current
    const close = (e) => {
      if (ref.current && ref.current.contains(e.target)) return
      // The chip that opened this menu handles a second press itself. Closing
      // here would run first, and the click would open the menu again.
      if (e.target.closest?.('[data-picker]')) return
      // In the capture phase, because React delegates its handlers at the root: a
      // dialog's backdrop close ran before a bubble-phase window listener in the same
      // press, so clicking the backdrop to dismiss this menu also closed the dialog
      // that hosted it. The menu takes the press and stops it here.
      e.stopPropagation()
      onClose()
    }
    window.addEventListener('mousedown', close, true)
    return () => {
      window.removeEventListener('mousedown', close, true)
      if (returnTo?.isConnected) returnTo.focus?.({ preventScroll: true })
    }
  }, [onClose])

  useEffect(() => {
    setCursor(Math.max(0, filtered.findIndex((i) => i.id === selectedId)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query])

  // keep the menu inside the viewport
  useLayoutEffect(() => {
    if (!ref.current) return undefined
    const el = ref.current
    // A function, run again on a resize: the clamp used to run per render only, so a
    // menu open while the window shrank hung past the edge with no scroll to reach the
    // rest of it. It is named apart from the `place` prop it reads, which decides
    // whether the menu hangs above its trigger or below it.
    const keepInView = () => {
      if (!el.isConnected) return
      const viewport = window.visualViewport
      el.style.maxHeight = `min(380px, ${Math.max(0, (viewport?.height ?? window.innerHeight) - 16)}px)`
      const size = el.getBoundingClientRect()
      const { left, top } = fitPicker(
        getAnchor?.() ?? spot,
        { width: size.width, height: size.height },
        {
          width: viewport?.width ?? window.innerWidth,
          height: viewport?.height ?? window.innerHeight,
          offsetTop: viewport?.offsetTop ?? 0,
        },
        place,
      )
      el.style.left = `${left}px`
      el.style.top = `${top}px`
    }
    keepInView()
    // Fonts and async rows can change the height after the first layout.
    const observer = new ResizeObserver(keepInView)
    observer.observe(el)
    window.addEventListener('resize', keepInView)
    window.visualViewport?.addEventListener('resize', keepInView)
    window.visualViewport?.addEventListener('scroll', keepInView)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', keepInView)
      window.visualViewport?.removeEventListener('resize', keepInView)
      window.visualViewport?.removeEventListener('scroll', keepInView)
    }
  })

  useEffect(() => {
    ref.current?.querySelector('.nc-item.cursor')?.scrollIntoView({ block: 'nearest' })
  }, [cursor])

  // Keys are handled at the menu root, not on the search input: Tab can
  // move focus to the footer row, and Escape / arrows / Enter keep
  // working from wherever focus sits inside the menu. Enter on a
  // focused footer button is its own native click - the item Enter
  // here only fires when the input still holds focus.
  const onKeyDown = (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation()
      onClose()
      return
    }
    if (!hideSearch && e.target !== searchRef.current) return
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setCursor((c) => Math.min(filtered.length - 1, c + 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setCursor((c) => Math.max(0, c - 1))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const item = filtered[cursor]
      if (item) onChoose(item.id)
    }
  }

  const grouped = groups
    ? (() => {
        const byLabel = new Map()
        for (const item of filtered) {
          const g = item.group ?? ''
          if (!byLabel.has(g)) byLabel.set(g, [])
          byLabel.get(g).push(item)
        }
        return [...byLabel.entries()].map(([label, items]) => ({ label, items }))
      })()
    : [{ label: null, items: filtered }]

  let flatIndex = -1

  // The empty state reads differently by cause: a search that found
  // nothing names the query; a list with nothing in it yet says so
  // plainly ("no harnesses yet" is the moment the footer row carries
  // the action). A label ending in a consonant and a y takes "ies".
  const plural = (word) => {
    const lower = word.toLowerCase()
    if (lower.endsWith('s')) return `${lower}es`
    if (/[^aeiou]y$/.test(lower)) return `${lower.slice(0, -1)}ies`
    return `${lower}s`
  }
  const emptyLine = query.trim()
    ? `No ${plural(label)} match "${query}"`
    : `No ${plural(label)} yet`

  const menu = (
    <div
      ref={ref}
      className={`nc-menu${isMobile ? ' is-phone' : ''}${wide ? ' nc-menu--wide' : ''}${leaving ? ' is-out' : ''}${place === 'above' ? ' is-above' : ''}`}
      role="listbox"
      tabIndex={-1}
      aria-label={label}
      style={spot}
      onKeyDown={onKeyDown}
    >
      {!hideSearch && <label className="nc-search">
        <MagnifyingGlass size={13} />
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value)
            setCursor(0)
          }}
          placeholder={searchPlaceholder}
          spellCheck={false}
        />
      </label>}
      {note && <p className="nc-note">{note}</p>}
      <div className="nc-items">
        {grouped.map((group) => (
          <div key={group.label ?? 'all'} className="nc-group">
            {group.label && (
              <div className="nc-group-label">
                {group.label === 'Installed' && <ClockCounterClockwise size={10} />}
                {group.label}
              </div>
            )}
            {group.items.map((item) => {
              flatIndex++
              const i = flatIndex
              return (
                <button
                  key={item.id}
                  className={`nc-item${item.detail ? ' has-detail' : ''}${
                    item.id === selectedId ? ' selected' : ''
                  }${i === cursor ? ' cursor' : ''}`}
                  role="option"
                  aria-selected={item.id === selectedId}
                  title={item.note || undefined}
                  onClick={() => onChoose(item.id)}
                  onMouseEnter={() => setCursor(i)}
                >
                  <span className="nc-item-main">
                    {renderIcon?.(item)}
                    {/* an item marked veiled carries the user's own name for a thing
                        (a project, a machine): privacy mode blurs it */}
                    <span className={item.veiled ? 'nc-item-name pv' : 'nc-item-name'}>{item.name}</span>
                    {/* the catalog's second fact about the row (a model's cost
                        tier): ordered after the marks so it takes the full
                        width under them, see nc-item-detail */}
                    {item.detail && <span className="nc-item-detail">{item.detail}</span>}
                    {renderTrailing?.(item)}
                    {item.id === selectedId && <Check size={13} className="nc-check" />}
                  </span>
                  {renderSubline?.(item)}
                </button>
              )
            })}
          </div>
        ))}
        {filtered.length === 0 && <div className="nc-empty">{emptyLine}</div>}
      </div>
      {footer && <div className="nc-footer">{footer}</div>}
    </div>
  )
  // The phone sheet moves over its sidebar. Its popups must escape that
  // stacking context; desktop menus stay inside their dialog's focus scope.
  return portal ? createPortal(menu, document.body) : menu
}

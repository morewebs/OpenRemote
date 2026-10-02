import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Check, ClockCounterClockwise, MagnifyingGlass } from '@phosphor-icons/react'
import './pickermenu.css'

/**
 * The cozy dropdown engine shared by the model / harness / device pickers.
 * Opens at the cursor, searches, groups (optional), keyboard-navigates,
 * clamps to the viewport, scrolls internally.
 */
export default function PickerMenu({
  label,
  searchPlaceholder,
  items,
  groups, // [{ label, items }] or null for a flat list
  selectedId,
  onChoose,
  onClose,
  anchor, // { x, y }
  renderIcon,
  renderTrailing,
  renderSubline, // a muted second line inside the item (efforts, paths)
  wide, // room for sublines without cramped trailing text
}) {
  const [query, setQuery] = useState('')
  const [cursor, setCursor] = useState(0)
  const ref = useRef(null)
  const searchRef = useRef(null)

  const filtered = (() => {
    const q = query.trim().toLowerCase()
    if (!q) return items
    return items.filter((i) => i.name.toLowerCase().includes(q))
  })()

  useEffect(() => {
    searchRef.current?.focus()
    const close = (e) => {
      if (ref.current && ref.current.contains(e.target)) return
      onClose()
    }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [onClose])

  useEffect(() => {
    setCursor(Math.max(0, filtered.findIndex((i) => i.id === selectedId)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query])

  // keep the menu inside the viewport
  useLayoutEffect(() => {
    if (!ref.current) return
    const el = ref.current
    const vw = window.innerWidth
    const vh = window.innerHeight
    const rect = el.getBoundingClientRect()
    let { left, top } = rect
    let maxHeight = ''
    if (rect.bottom > vh - 8) {
      top = Math.max(8, vh - rect.height - 8)
      if (top + rect.height > vh - 8) {
        top = 8
        maxHeight = `${vh - 16}px`
      }
    }
    if (rect.right > vw - 8) left = Math.max(8, vw - rect.width - 8)
    el.style.left = `${left}px`
    el.style.top = `${top}px`
    el.style.maxHeight = maxHeight
  })

  useEffect(() => {
    ref.current?.querySelector('.nc-item.cursor')?.scrollIntoView({ block: 'nearest' })
  }, [cursor])

  const onKeyDown = (e) => {
    if (e.key === 'Escape') {
      e.stopPropagation()
      onClose()
      return
    }
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

  return (
    <div
      ref={ref}
      className={`nc-menu${wide ? ' nc-menu--wide' : ''}`}
      role="listbox"
      aria-label={label}
      style={anchor}
    >
      <label className="nc-search">
        <MagnifyingGlass size={13} />
        <input
          ref={searchRef}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value)
            setCursor(0)
          }}
          onKeyDown={onKeyDown}
          placeholder={searchPlaceholder}
          spellCheck={false}
        />
      </label>
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
                  className={`nc-item${item.id === selectedId ? ' selected' : ''}${
                    i === cursor ? ' cursor' : ''
                  }`}
                  role="option"
                  aria-selected={item.id === selectedId}
                  onClick={() => onChoose(item.id)}
                  onMouseEnter={() => setCursor(i)}
                >
                  <span className="nc-item-main">
                    {renderIcon?.(item)}
                    <span className="nc-item-name">{item.name}</span>
                    {renderTrailing?.(item)}
                    {item.id === selectedId && <Check size={13} className="nc-check" />}
                  </span>
                  {renderSubline?.(item)}
                </button>
              )
            })}
          </div>
        ))}
        {filtered.length === 0 && (
          <div className="nc-empty">No {label.toLowerCase()}s match "{query}"</div>
        )}
      </div>
    </div>
  )
}

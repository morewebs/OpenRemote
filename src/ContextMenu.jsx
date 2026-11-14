import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { CaretRight } from '@phosphor-icons/react'
import { captureOpener, focusOpener } from './dialog-focus.js'
import { useBack } from './use-back.js'
import { isMobile } from './platform.js'
import { fitPicker } from './picker-anchor.js'
import { SUBMENU_LINGER_MS, menuActions, stepCursor, submenuPlace } from './context-menu.js'
import './pickermenu.css'
import './contextmenu.css'

/**
 * The right-click menu: actions at the cursor. It wears the picker menu's
 * look (nc-* classes) but has no search and no selection - each entry just
 * does its thing and the menu closes.
 *
 * items: [{ label, icon, onSelect, disabled, hint, danger }], a { header: 'Text' }
 * for a group label, { divider: true }, or { label, icon, submenu: [items] }:
 * a row whose own list opens beside the menu - on hover, a click or a tap,
 * or the right arrow (the left arrow or Esc goes back). The rules live in
 * context-menu.js.
 */
export default function ContextMenu({ x, y, items, onClose }) {
  const ref = useRef(null)
  const subRef = useRef(null)
  const rows = useRef([])
  const linger = useRef(null)
  // The listeners below outlive renders; re-arming them on each one would
  // also pull the focus back out of an open submenu.
  const close = useRef(onClose)
  close.current = onClose
  // On a phone, where a long press opens it, back closes it.
  useBack(true, onClose)
  const actions = menuActions(items)
  const [cursor, setCursor] = useState(actions[0]?.i ?? -1)
  // The row whose submenu is open, and the cursor inside that submenu.
  const [open, setOpen] = useState(null)
  const [subCursor, setSubCursor] = useState(-1)
  const sub = open == null ? null : (items[open]?.submenu ?? null)
  const subActions = (sub ?? []).map((item, i) => ({ item, i })).filter(({ item }) => item.onSelect && !item.disabled)

  useEffect(() => {
    // The menu takes the keyboard while it is open and hands it back when it
    // goes - Escape, a chosen row, a click elsewhere, or the opener
    // unmounting. Captured before the focus below, which is the last moment
    // the opener is the active element; the shared helper finds the control
    // again when the row it was has been re-rendered under the menu. Once,
    // on open: a re-render must not pull the focus back out of a submenu.
    const opener = captureOpener(typeof document === 'undefined' ? null : document.activeElement)
    ref.current?.focus()
    return () => {
      // A row that opens a dialog focuses it in that dialog's own effect, so
      // this hands the keyboard back before the dialog claims it.
      focusOpener(opener)
    }
  }, [])

  useEffect(() => {
    const inside = (target) => ref.current?.contains(target) || subRef.current?.contains(target)
    const away = (e) => {
      if (!inside(e.target)) close.current()
    }
    const shut = () => close.current()
    // Anything that moves the page under the cursor strands the menu - but a
    // long submenu scrolls its own list. The listeners wait a tick: the
    // right-click that opened the menu is still bubbling and must not count
    // as a click elsewhere.
    const wheel = (e) => {
      if (!inside(e.target)) close.current()
    }
    const listeners = [
      ['mousedown', away],
      ['contextmenu', away],
      ...(!isMobile ? [['resize', shut]] : []),
      ['blur', shut],
      ['wheel', wheel, { passive: true }],
    ]
    const arm = setTimeout(() => {
      for (const [type, fn, opts] of listeners) window.addEventListener(type, fn, opts)
    }, 0)
    return () => {
      clearTimeout(arm)
      clearTimeout(linger.current)
      for (const [type, fn] of listeners) window.removeEventListener(type, fn)
    }
  }, [])

  // Open at the press, within the visible viewport even with a phone keyboard.
  // Both lists can outgrow the screen, so constrain them before measuring.
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const list = subRef.current
    const fit = () => {
      const viewport = window.visualViewport
      const width = viewport?.width ?? window.innerWidth
      const height = viewport?.height ?? window.innerHeight
      const offsetTop = viewport?.offsetTop ?? 0
      el.style.maxHeight = `${Math.max(0, height - 16)}px`
      const at = fitPicker({ left: x, top: y }, el.getBoundingClientRect(), { width, height, offsetTop })
      el.style.left = `${at.left}px`
      el.style.top = `${at.top}px`
      const row = rows.current[open]
      if (!list || !row) return
      list.style.maxHeight = `${Math.max(0, height - 16)}px`
      const subAt = submenuPlace({
        menu: el.getBoundingClientRect(),
        row: row.getBoundingClientRect(),
        list: list.getBoundingClientRect(),
        vw: width,
        vh: height,
        offsetTop,
      })
      list.style.left = `${subAt.left}px`
      list.style.top = `${subAt.top}px`
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(el)
    if (list) observer.observe(list)
    // Scrolling the parent list must keep a submenu beside its own row.
    el.addEventListener('scroll', fit)
    window.addEventListener('resize', fit)
    window.visualViewport?.addEventListener('resize', fit)
    window.visualViewport?.addEventListener('scroll', fit)
    return () => {
      observer.disconnect()
      el.removeEventListener('scroll', fit)
      window.removeEventListener('resize', fit)
      window.visualViewport?.removeEventListener('resize', fit)
      window.visualViewport?.removeEventListener('scroll', fit)
    }
  }, [x, y, open])

  const run = (item) => {
    if (item.disabled) return
    onClose()
    item.onSelect()
  }

  const openSub = (i, byKeys) => {
    clearTimeout(linger.current)
    setOpen(i)
    const first = (items[i]?.submenu ?? []).findIndex((item) => item.onSelect && !item.disabled)
    setSubCursor(byKeys ? first : -1)
    // The keys move into the list; the pointer leaves the focus where it is.
    if (byKeys) requestAnimationFrame(() => subRef.current?.focus())
  }

  useEffect(() => {
    rows.current[cursor]?.scrollIntoView({ block: 'nearest' })
  }, [cursor])

  useEffect(() => {
    subRef.current?.querySelector('.nc-item.cursor')?.scrollIntoView({ block: 'nearest' })
  }, [open, subCursor])

  const onKeyDown = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      onClose()
      return
    }
    if (actions.length === 0) return
    const at = actions.findIndex(({ i }) => i === cursor)
    const next = stepCursor(actions, at, e.key)
    if (next != null) {
      e.preventDefault()
      setCursor(next)
      if (next !== open) setOpen(null)
    } else if (e.key === 'ArrowRight' && items[cursor]?.submenu) {
      e.preventDefault()
      openSub(cursor, true)
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      if (at === -1) return
      if (items[cursor].submenu) openSub(cursor, true)
      else run(items[cursor])
    }
  }

  const onSubKeyDown = (e) => {
    e.stopPropagation()
    if (e.key === 'Escape' || e.key === 'ArrowLeft') {
      e.preventDefault()
      setOpen(null)
      ref.current?.focus()
      return
    }
    if (subActions.length === 0) return
    const at = subActions.findIndex(({ i }) => i === subCursor)
    const next = stepCursor(subActions, at, e.key)
    if (next != null) {
      e.preventDefault()
      setSubCursor(next)
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      if (at !== -1) run(sub[subCursor])
    }
  }

  const row = (item, i, { current, onEnter, onClick, keep }) => {
    if (item.divider) return <div key={i} className="ctx-sep" role="separator" />
    if (item.header) {
      return (
        <div key={i} className="nc-group-label">
          {item.header}
        </div>
      )
    }
    return (
      <button
        key={i}
        ref={keep}
        type="button"
        role="menuitem"
        className={`nc-item${current ? ' cursor' : ''}${item.danger ? ' ctx-danger' : ''}`}
        disabled={item.disabled}
        aria-haspopup={item.submenu ? 'menu' : undefined}
        aria-expanded={item.submenu ? open === i : undefined}
        onClick={onClick}
        onMouseEnter={onEnter}
      >
        <span className="nc-item-main">
          {item.icon && <span className="ctx-icon">{item.icon}</span>}
          <span className="nc-item-name">{item.label}</span>
          {item.hint && <span className="ctx-hint">{item.hint}</span>}
          {item.submenu && (
            <span className="ctx-caret" aria-hidden="true">
              <CaretRight size={11} weight="bold" />
            </span>
          )}
        </span>
      </button>
    )
  }

  // In the body, so no ancestor's transform or clipping can displace it.
  return (
    <>
      {isMobile && createPortal(
        <div
          className="ctx-backdrop"
          aria-hidden="true"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation()
            onClose()
          }}
        />,
        document.body,
      )}
      {createPortal(
        <div
          ref={ref}
          className={`nc-menu ctx-menu${isMobile ? ' is-phone' : ''}`}
          role="menu"
          tabIndex={-1}
          style={{ left: x, top: y }}
          onKeyDown={onKeyDown}
          onPointerDown={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.preventDefault()}
        >
          {items.map((item, i) =>
            row(item, i, {
              current: i === cursor,
              keep: (el) => {
                rows.current[i] = el
              },
              onEnter: () => {
                if (item.disabled) return
                setCursor(i)
                if (item.submenu) {
                  if (open !== i) openSub(i, false)
                  else clearTimeout(linger.current)
                } else if (open != null) {
                  clearTimeout(linger.current)
                  linger.current = setTimeout(() => {
                    // Focus inside the closing list comes back to the menu,
                    // so Esc and the arrows keep working.
                    if (subRef.current?.contains(document.activeElement)) ref.current?.focus()
                    setOpen(null)
                  }, SUBMENU_LINGER_MS)
                }
              },
              // A tap opens a submenu, never closes it: on a touchscreen the
              // same tap already arrived as a hover.
              onClick: () => (item.submenu ? openSub(i, false) : run(item)),
            }),
          )}
        </div>,
        document.body,
      )}
      {sub &&
        createPortal(
          <div
            ref={subRef}
            className={`nc-menu ctx-menu ctx-sub${isMobile ? ' is-phone' : ''}`}
            role="menu"
            tabIndex={-1}
            onKeyDown={onSubKeyDown}
            onPointerDown={(e) => e.stopPropagation()}
            onMouseEnter={() => clearTimeout(linger.current)}
            onContextMenu={(e) => e.preventDefault()}
          >
            {sub.map((item, j) =>
              row(item, j, {
                current: j === subCursor,
                onEnter: () => {
                  clearTimeout(linger.current)
                  if (!item.disabled) setSubCursor(j)
                },
                onClick: () => run(item),
              }),
            )}
          </div>,
          document.body,
        )}
    </>
  )
}

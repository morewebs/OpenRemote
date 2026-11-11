// The bookmarks manager (ported from the browser branch's openremote://
// bookmarks page, 0d3211a): both folders, searchable, in the pane's dialog
// family like its history rather than a page of its own. A row opens its
// address (Ctrl+click or middle click in a new tab); drag to reorder or move
// between folders, or focus a row and use Alt+Up and Alt+Down; F2 renames,
// Delete removes. Every change writes through browser-store.js, so the
// daemon keeps the rows and a refusal puts the list back with the bar's
// stale note.

import { useEffect, useRef, useState } from 'react'
import { ArrowDown, ArrowUp, DotsSixVertical, DotsThreeVertical, FolderSimple, MagnifyingGlass, PencilSimple, Plus, Trash, X } from '@phosphor-icons/react'
import ContextMenu from './ContextMenu.jsx'
import { FOLDERS, inFolder, labelFor, matching, otherFolder, placeAt } from './browser-bookmarks.js'
import { bareAddress } from './browser-omnibox.js'
import { useDialog } from './dialog-focus.js'
import { useBack } from './use-back.js'
import { usePrivacyMode } from './privacy.js'
import './devices.css'
import './panels.css'

function MarkRow({ mark, renaming, dropBefore, dragging, drag, onOpen, onRename, onKey, onMenu }) {
  const privacy = usePrivacyMode()
  const [title, setTitle] = useState(mark.title ?? '')
  useEffect(() => setTitle(mark.title ?? ''), [mark.title, renaming])
  return (
    <div
      className={`bw-mark-row${dropBefore ? ' drop' : ''}${dragging ? ' dragging' : ''}`}
      tabIndex={0}
      data-mark={mark.id}
      draggable={!renaming && drag.enabled}
      onDragStart={(e) => drag.start(e, mark)}
      onDragEnd={drag.end}
      onDragOver={(e) => drag.over(e, mark)}
      onDrop={drag.drop}
      onKeyDown={(e) => onKey(e, mark)}
      aria-label={privacy ? 'Bookmark' : labelFor(mark)}
    >
      {drag.enabled && <DotsSixVertical size={14} className="bw-mark-grip" aria-hidden="true" />}
      {renaming ? (
        <input
          className="bw-field-input bw-mark-rename"
          value={title}
          autoFocus
          spellCheck={false}
          aria-label="Bookmark name"
          onFocus={(e) => e.target.select()}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() => onRename(mark, title)}
          onKeyDown={(e) => {
            e.stopPropagation()
            if (e.key === 'Enter') {
              e.preventDefault()
              onRename(mark, title)
            } else if (e.key === 'Escape') {
              // This Esc ends the rename, not the dialog.
              e.preventDefault()
              onRename(mark, null)
            }
          }}
        />
      ) : (
        <button
          type="button"
          className="bw-mark-open"
          tabIndex={-1}
          title={privacy ? undefined : mark.url}
          onMouseDown={(e) => {
            if (e.button === 1) e.preventDefault()
          }}
          onClick={(e) => onOpen(mark.url, { newTab: e.ctrlKey || e.metaKey })}
          onAuxClick={(e) => {
            if (e.button === 1) onOpen(mark.url, { newTab: true })
          }}
        >
          <span className="bw-history-title pv">{labelFor(mark)}</span>
          <span className="bw-history-url pv">{bareAddress(mark.url)}</span>
        </button>
      )}
      <button type="button" className="bw-btn bw-mark-more" title="More" aria-label="More" onClick={(e) => onMenu(e, mark)}>
        <DotsThreeVertical size={14} weight="bold" />
      </button>
    </div>
  )
}

export default function BrowserBookmarksModal({ marks, onOpen, onUpdate, onRemove, onClose }) {
  const panelRef = useRef(null)
  const [q, setQ] = useState('')
  const [renaming, setRenaming] = useState(null)
  const [menu, setMenu] = useState(null)
  const [dragId, setDragId] = useState(null)
  const [over, setOver] = useState(null)
  useDialog(panelRef, onClose)
  useBack(true, onClose)
  const filter = q.trim()

  // A bookmark dropped at `index` of `folder` (counted as the folder shows
  // now, the moving one included): only that bookmark is written.
  const moveTo = (mark, folder, index) => onUpdate(mark.id, placeAt(marks, mark, folder, index))

  const step = (mark, dir) => {
    const list = inFolder(marks, mark.folder === 'other' ? 'other' : 'bar')
    const at = list.findIndex((m) => m.id === mark.id)
    const to = at + dir
    if (at === -1 || to < 0 || to >= list.length) return
    // Past the neighbour: one further when moving down, as the drop counts.
    moveTo(mark, mark.folder === 'other' ? 'other' : 'bar', dir > 0 ? to + 1 : to)
    // The row keeps the keyboard where it went.
    requestAnimationFrame(() => panelRef.current?.querySelector(`[data-mark="${CSS.escape(mark.id)}"]`)?.focus())
  }

  const rename = (mark, title) => {
    setRenaming(null)
    if (title == null) return
    const clean = title.trim()
    if (clean && clean !== mark.title) onUpdate(mark.id, { title: clean })
  }

  const onKey = (e, mark) => {
    if (e.target !== e.currentTarget) return
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault()
      step(mark, e.key === 'ArrowUp' ? -1 : 1)
    } else if (e.key === 'F2') {
      e.preventDefault()
      setRenaming(mark.id)
    } else if (e.key === 'Delete') {
      e.preventDefault()
      onRemove(mark.id)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      onOpen(mark.url, { newTab: e.ctrlKey || e.metaKey })
    }
  }

  const drag = (folder) => ({
    // Reordering a filtered list would drop between rows the person can't see.
    enabled: !filter,
    start: (e, mark) => {
      e.dataTransfer.effectAllowed = 'move'
      e.dataTransfer.setData('text/plain', mark.url)
      setDragId(mark.id)
    },
    end: () => {
      setDragId(null)
      setOver(null)
    },
    over: (e, mark) => {
      if (!dragId) return
      e.preventDefault()
      e.stopPropagation()
      const list = inFolder(marks, folder)
      const r = e.currentTarget.getBoundingClientRect()
      const at = list.findIndex((m) => m.id === mark.id)
      const index = e.clientY > r.top + r.height / 2 ? at + 1 : at
      if (over?.folder !== folder || over?.index !== index) setOver({ folder, index })
    },
    drop: (e) => {
      e.preventDefault()
      e.stopPropagation()
      const mark = marks.find((m) => m.id === dragId)
      if (mark && over) moveTo(mark, over.folder, over.index)
      setDragId(null)
      setOver(null)
    },
  })

  const menuItems = (mark) => {
    const list = inFolder(marks, mark.folder === 'other' ? 'other' : 'bar')
    const at = list.findIndex((m) => m.id === mark.id)
    const there = otherFolder(mark)
    return [
      { label: 'Open in a new tab', icon: <Plus size={14} />, onSelect: () => onOpen(mark.url, { newTab: true }) },
      { label: 'Rename', icon: <PencilSimple size={14} />, hint: 'F2', onSelect: () => setRenaming(mark.id) },
      { divider: true },
      { label: 'Move up', icon: <ArrowUp size={14} />, hint: 'Alt+Up', disabled: at <= 0, onSelect: () => step(mark, -1) },
      { label: 'Move down', icon: <ArrowDown size={14} />, hint: 'Alt+Down', disabled: at === list.length - 1, onSelect: () => step(mark, 1) },
      {
        label: `Move to ${FOLDERS[there]}`,
        icon: <FolderSimple size={14} />,
        onSelect: () => moveTo(mark, there, inFolder(marks, there).length),
      },
      { divider: true },
      { label: 'Delete', icon: <Trash size={14} />, hint: 'Del', danger: true, onSelect: () => onRemove(mark.id) },
    ]
  }

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="dv-modal bw-history bw-marks-manager"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="Bookmarks"
      >
        <div className="dv-modal-head">
          <span className="dv-modal-title">Bookmarks</span>
          <button type="button" className="dv-modal-close" aria-label="Close bookmarks" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        <label className="bw-marks-search">
          <MagnifyingGlass size={14} />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search bookmarks" spellCheck={false} aria-label="Search bookmarks" />
        </label>
        <p className="bw-marks-lead">Drag to reorder, or focus a bookmark and use Alt+Up and Alt+Down. F2 renames, Delete removes.</p>
        <div className="bw-history-list">
          {Object.entries(FOLDERS).map(([folder, label]) => {
            const list = matching(marks, folder, filter)
            const all = inFolder(marks, folder)
            const handlers = drag(folder)
            return (
              <section
                key={folder}
                className={`bw-history-day${over?.folder === folder && over.index >= all.length ? ' drop-end' : ''}`}
                aria-label={label}
                onDragOver={(e) => {
                  if (!dragId) return
                  e.preventDefault()
                  // Past the last row, or anywhere in an empty folder: the end.
                  if ((!all.length || e.target === e.currentTarget) && (over?.folder !== folder || over?.index !== all.length)) {
                    setOver({ folder, index: all.length })
                  }
                }}
                onDrop={handlers.drop}
              >
                <h3 className="bw-history-label">{label}</h3>
                {list.map((m) => (
                  <MarkRow
                    key={m.id}
                    mark={m}
                    renaming={renaming === m.id}
                    dropBefore={over?.folder === folder && all[over.index]?.id === m.id && dragId !== m.id}
                    dragging={dragId === m.id}
                    drag={handlers}
                    onOpen={onOpen}
                    onRename={rename}
                    onKey={onKey}
                    onMenu={(e, mark) => {
                      const r = e.currentTarget.getBoundingClientRect()
                      setMenu({ mark, x: r.left, y: r.bottom + 4 })
                    }}
                  />
                ))}
                {!list.length && (
                  <p className="bw-history-empty">
                    {filter ? 'None match.' : folder === 'bar' ? 'Bookmarks you add with the star land here.' : 'Nothing here yet.'}
                  </p>
                )}
              </section>
            )
          })}
        </div>
        <div className="ctx-modal-actions">
          <button type="button" className="pn-btn bw-primary" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.mark)} onClose={() => setMenu(null)} />}
    </div>
  )
}

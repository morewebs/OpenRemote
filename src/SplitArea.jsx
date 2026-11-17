import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, ArrowsIn, ArrowsOut, DotsSixVertical, X } from '@phosphor-icons/react'
import ContextMenu from './ContextMenu.jsx'
import WorkspaceLayouts from './WorkspaceLayouts.jsx'
import { HarnessMark } from './brand-marks.jsx'
import { useKeyboard } from './keyboard.jsx'
import { useEscLayer } from './esc-layer.js'
import { RECONNECTING, paneChatId, paneHeader } from './pane-header.js'
import { projectFor } from './projects.js'
import { useConsole } from './state/console.jsx'
import { acceptsChatDrop, balanceSplit, draggedChat, dropZone, focusLeaf, leafById, leaves, MAX_PANES, splitLeaf } from './split.js'
import { dockPane, equalizeWorkspace, layoutFits, paneForView, presetLayout, resizeWorkspaceDivider, sidebarDraggedChat, workspaceGeometry, workspaceGestureValid } from './workspace-layout.js'
import './split.css'

const position = r => ({ left: `${r.x * 100}%`, top: `${r.y * 100}%`, width: `${r.w * 100}%`, height: `${r.h * 100}%` })
const actionLabel = zone => ({ center: 'Swap panes', left: 'Dock left', right: 'Dock right', top: 'Dock above', bottom: 'Dock below' })[zone]

function Pane({ node, rect, focused, hidden, zoomed, showHeader, labelFor, renderPane, currentChat, onFocus, onStartDrag, onZoom, onClose, onSplit, onDropInPane, layout, room, canSplit, onPreview, onClearPreview }) {
  const [menu, setMenu] = useState(null)
  const dropRef = useRef(null)
  const label = labelFor(node)
  const { chats, projects, reconnecting } = useConsole()
  const chatId = paneChatId(node.view, currentChat)
  const chat = chatId ? (chats?.[chatId] ?? null) : null
  const head = paneHeader({ label, chat, project: chat ? projectFor(projects ?? [], chat.workspace) : null,
    reconnecting: chatId ? Boolean(reconnecting?.[chatId]) : false,
  })
  const kb = useKeyboard()
  const over = e => {
    if (!onDropInPane || !acceptsChatDrop(e.dataTransfer.types)) return
    e.preventDefault()
    const id = sidebarDraggedChat()
    const source = id ? paneForView(layout, currentChat, id) : null
    const zone = zoomed ? { zone: 'center' } : dropZone(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY)
    let candidate = null
    if (zone.zone !== 'center') {
      candidate = source ? dockPane(layout, source.id, node.id, zone.zone) : canSplit ? splitLeaf(layout, node.id, zone.dir, id ?? 'new', zone.before) : null
      if (!candidate || candidate === layout || !layoutFits(candidate, room.width, room.height)) {
        dropRef.current = { zone: 'center' }
        onPreview({ rect: zoomed ? { x: 0, y: 0, w: 1, h: 1 } : source ? workspaceGeometry(layout).panes.find(p => p.id === source.id) : rect, label: source ? 'Focus this chat' : 'Open here' })
        return
      }
    }
    dropRef.current = zone
    const dest = zoomed ? { x: 0, y: 0, w: 1, h: 1 } : candidate ? workspaceGeometry(candidate).panes.find(p => p.id === (source?.id ?? candidate.focus)) : source ? workspaceGeometry(layout).panes.find(p => p.id === source.id) : rect
    onPreview({ rect: dest, label: zone.zone === 'center' ? source ? 'Focus this chat' : 'Open here' : actionLabel(zone.zone) })
  }
  const leave = e => {
    if (e.relatedTarget && e.currentTarget.contains(e.relatedTarget)) return
    dropRef.current = null
    onClearPreview()
  }
  const drop = e => {
    const id = draggedChat(e.dataTransfer)
    const zone = dropRef.current ?? { zone: 'center' }
    dropRef.current = null
    onClearPreview()
    if (!id) return
    e.preventDefault()
    onDropInPane?.(node.id, id, zone)
  }
  const items = [
    { label: zoomed ? 'Restore panes' : 'Maximize pane', icon: <ArrowsOut size={18} />, onSelect: onZoom },
    { divider: true },
    ...[['right', 'row', false, ArrowRight], ['down', 'col', false, ArrowDown], ['left', 'row', true, ArrowLeft], ['up', 'col', true, ArrowUp]].map(([name, dir, before, Icon]) => ({
      label: `Split ${name}`, icon: <Icon size={18} />, hint: kb?.label(`split-${name}`),
      disabled: !canSplit || !layoutFits(splitLeaf(layout, node.id, dir, 'new', before), room.width, room.height),
      onSelect: () => onSplit(node.id, dir, before),
    })),
    { divider: true },
    { label: 'Move pane', disabled: !showHeader, submenu: leaves(layout.root).filter(p => p.id !== node.id).map(p => ({ label: `Swap with ${labelFor(p)}`, onSelect: () => onSplit(node.id, null, p.id) })) },
    ...(showHeader ? [{ divider: true }, { label: 'Close pane', icon: <X size={18} />, hint: kb?.label('close-pane'), onSelect: onClose }] : []),
  ]
  return <section className={`split-pane${focused ? ' focused' : ''}${zoomed ? ' is-maximized' : ''}${menu ? ' has-menu' : ''}`}
    data-leaf-id={node.id} style={{ ...position(rect), display: hidden ? 'none' : undefined }} inert={hidden || undefined}
    onPointerDownCapture={onFocus} onFocusCapture={onFocus} aria-label={label}>
    {showHeader && <header className="split-bar">
      <div className="split-label" title={`${head.title} - drag to dock or swap; double-click to maximize`}
        tabIndex={0} role="button" aria-label={`Move ${head.title}`} onPointerDown={e => onStartDrag(e, node.id)} onDoubleClick={onZoom}
        onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); onZoom() } }}
        onContextMenu={e => { e.preventDefault(); e.stopPropagation(); setMenu({ x: e.clientX, y: e.clientY }) }}>
        <DotsSixVertical size={14} className="split-grip" />
        {head.chat && <span className={`dot dot--${head.status}`} aria-hidden="true" />}
        {head.chat && head.harness && <HarnessMark harness={head.harness} size={16} />}
        <span className="split-title">{head.title}</span>
        {head.reconnecting && <span className="split-reconnecting">{RECONNECTING}</span>}
        {head.project && <span className="split-project">{head.project}</span>}
      </div>
      <div className="split-actions">
        <button type="button" className="split-btn" title={zoomed ? 'Restore panes' : 'Maximize pane'} aria-label={zoomed ? 'Restore panes' : 'Maximize pane'} onClick={onZoom}>{zoomed ? <ArrowsIn size={18} /> : <ArrowsOut size={18} />}</button>
        <button type="button" className="split-btn" title="Close this pane" aria-label="Close this pane" onClick={onClose}><X size={18} /></button>
      </div>
    </header>}
    <div className="split-body" onDragOver={over} onDragLeave={leave} onDrop={drop}>{renderPane(node)}</div>
    {menu && <ContextMenu x={menu.x} y={menu.y} items={items} compact label="Pane actions" onClose={() => setMenu(null)} />}
  </section>
}

/** Flat keyed panes stay mounted when the layout tree changes around them. */
export default function SplitArea({ layout, setLayout, renderPane, labelFor, onDropInPane, canSplit = true, onSaveLayout = null, onRestoreLayout = null, currentChat = null, onClosePane = null, layoutControl = null, onCloseLayoutControl = null, onRoomChange = null }) {
  const areaRef = useRef(null)
  const latest = useRef(layout)
  latest.current = layout
  const [room, setRoom] = useState({ width: 0, height: 0 })
  const [zoom, setZoom] = useState(null)
  const [preview, setPreview] = useState(null)
  const [dragging, setDragging] = useState(null)
  const [resizing, setResizing] = useState(false)
  const [dragArmed, setDragArmed] = useState(false)
  const ghostRef = useRef(null)
  const gesture = useRef(null)
  const ordered = leaves(layout.root)
  const count = ordered.length
  const geometry = workspaceGeometry(layout)
  const zoomId = ordered.some(p => p.id === zoom) && count > 1 ? zoom : null
  const dragLeaf = dragging ? leafById(layout, dragging.id) : null
  const clearPreview = useCallback(() => setPreview(null), [])
  useLayoutEffect(() => {
    if (zoomId && zoomId !== layout.focus) setZoom(layout.focus)
  }, [layout.focus, zoomId])
  useLayoutEffect(() => {
    const current = gesture.current
    if (current && (!workspaceGestureValid(current.base, layout, current.pointerId, current.pointerId) || current.chat !== currentChat)) current.cancel()
  }, [layout.root, layout.focus, currentChat])
  useLayoutEffect(() => {
    const element = areaRef.current
    const measure = () => { gesture.current?.cancel(); const r = element.getBoundingClientRect(); const size = { width: r.width, height: r.height }; setRoom(size); onRoomChange?.(size) }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [onRoomChange])
  useEffect(() => {
    const clear = () => setPreview(null)
    window.addEventListener('dragend', clear)
    window.addEventListener('drop', clear)
    return () => { window.removeEventListener('dragend', clear); window.removeEventListener('drop', clear) }
  }, [])
  useEffect(() => () => gesture.current?.cancel(), [])
  useEscLayer(Boolean(dragArmed || resizing || zoomId), () => {
    if (gesture.current) gesture.current.cancel()
    else setZoom(null)
  })
  const paint = next => {
    const g = workspaceGeometry(next)
    for (const r of g.panes) {
      const el = areaRef.current?.querySelector(`[data-leaf-id="${r.id}"]`)
      if (el) Object.assign(el.style, position(r))
    }
    for (const r of g.dividers) {
      const el = areaRef.current?.querySelector(`[data-divider-id="${r.id}"]`)
      if (el) Object.assign(el.style, { left: `${r.x * 100}%`, top: `${r.y * 100}%`, width: r.dir === 'row' ? '9px' : `${r.w * 100}%`, height: r.dir === 'col' ? '9px' : `${r.h * 100}%` })
    }
  }
  const startResize = (e, divider) => {
    if (e.button !== 0) return
    e.preventDefault()
    const base = latest.current
    const box = areaRef.current.getBoundingClientRect()
    const row = divider.dir === 'row'
    const total = row ? box.width * divider.bounds.w : box.height * divider.bounds.h
    const start = row ? e.clientX : e.clientY
    const target = e.currentTarget
    const pointerId = e.pointerId
    let delta = 0
    let painted = base
    setResizing(true)
    target.setPointerCapture(e.pointerId)
    const move = ev => {
      if (ev.pointerId !== pointerId) return
      delta = ((row ? ev.clientX : ev.clientY) - start) / total
      painted = resizeWorkspaceDivider(base, divider, delta, box.width, box.height)
      paint(painted)
    }
    const finish = commit => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      target.removeEventListener('pointercancel', cancel)
      target.removeEventListener('lostpointercapture', cancel)
      window.removeEventListener('blur', cancel)
      gesture.current = null
      setResizing(false)
      if (commit && delta !== 0) setLayout(painted)
      else paint(latest.current)
    }
    const up = ev => {
      if (ev.pointerId !== pointerId) return
      if (!workspaceGestureValid(base, latest.current, ev.pointerId, pointerId)) return finish(false)
      move(ev)
      finish(true)
    }
    const cancel = ev => { if (!ev || ev.type === 'blur' || ev.pointerId === pointerId) finish(false) }
    gesture.current = { cancel, base, pointerId, chat: currentChat }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
    target.addEventListener('pointercancel', cancel)
    target.addEventListener('lostpointercapture', cancel)
    window.addEventListener('blur', cancel)
  }
  const startPaneDrag = (e, id) => {
    if (e.button !== 0 || zoomId || count < 2) return
    e.preventDefault()
    const base = focusLeaf(latest.current, id)
    const start = { x: e.clientX, y: e.clientY }
    const target = e.currentTarget
    const pointerId = e.pointerId
    setDragArmed(true)
    const box = areaRef.current.getBoundingClientRect()
    let started = false
    let next = null
    let lastTarget = null
    target.setPointerCapture(e.pointerId)
    const move = ev => {
      if (ev.pointerId !== pointerId) return
      if (!started && Math.hypot(ev.clientX - start.x, ev.clientY - start.y) < 7) return
      if (!started) setDragging({ id, x: ev.clientX - box.left + 14, y: ev.clientY - box.top + 14 })
      started = true
      if (ghostRef.current) { ghostRef.current.style.left = `${ev.clientX - box.left + 14}px`; ghostRef.current.style.top = `${ev.clientY - box.top + 14}px` }
      const x = (ev.clientX - box.left) / box.width
      const y = (ev.clientY - box.top) / box.height
      const hit = workspaceGeometry(base).panes.find(p => p.id !== id && x >= p.x && x <= p.x + p.w && y >= p.y && y <= p.y + p.h)
      next = null
      if (!hit) { if (lastTarget) setPreview(null); lastTarget = null; return }
      const at = dropZone({ left: hit.x * box.width + box.left, top: hit.y * box.height + box.top, right: (hit.x + hit.w) * box.width + box.left, bottom: (hit.y + hit.h) * box.height + box.top, width: hit.w * box.width, height: hit.h * box.height }, ev.clientX, ev.clientY)
      const candidate = dockPane(base, id, hit.id, at.zone)
      if (at.zone !== 'center' && !layoutFits(candidate, box.width, box.height)) { if (lastTarget) setPreview(null); lastTarget = null; return }
      next = candidate
      if (lastTarget === `${hit.id}:${at.zone}`) return
      lastTarget = `${hit.id}:${at.zone}`
      setPreview({ rect: workspaceGeometry(candidate).panes.find(p => p.id === id), label: actionLabel(at.zone), targets: hit, zone: at.zone })
    }
    const finish = commit => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      target.removeEventListener('pointercancel', cancel)
      target.removeEventListener('lostpointercapture', cancel)
      window.removeEventListener('blur', cancel)
      gesture.current = null
      setDragging(null)
      setDragArmed(false)
      setPreview(null)
      if (commit && next) setLayout(next)
    }
    const up = ev => {
      if (ev.pointerId !== pointerId) return
      if (!workspaceGestureValid(base, latest.current, ev.pointerId, pointerId)) return finish(false)
      if (started) move(ev)
      finish(true)
    }
    const cancel = ev => { if (!ev || ev.type === 'blur' || ev.pointerId === pointerId) finish(false) }
    gesture.current = { cancel, base, pointerId, chat: currentChat }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
    target.addEventListener('pointercancel', cancel)
    target.addEventListener('lostpointercapture', cancel)
    window.addEventListener('blur', cancel)
  }
  const split = (id, dir, before) => {
    setZoom(null)
    if (!dir) { setLayout(l => dockPane(l, id, before, 'center')); return }
    const candidate = splitLeaf(latest.current, id, dir, 'new', before)
    if (layoutFits(candidate, room.width, room.height)) setLayout(candidate)
  }
  const closePanel = useCallback(() => onCloseLayoutControl?.(), [onCloseLayoutControl])
  return <div ref={areaRef} className={`split-area${count > 1 ? ' is-split' : ''}${dragging ? ' is-dragging' : ''}`}>
    {ordered.map(node => <Pane key={node.id} node={node} rect={zoomId === node.id ? { x: 0, y: 0, w: 1, h: 1 } : geometry.panes.find(p => p.id === node.id)}
      focused={layout.focus === node.id} hidden={Boolean(zoomId && zoomId !== node.id)} zoomed={zoomId === node.id} showHeader={count > 1}
      labelFor={labelFor} renderPane={renderPane} currentChat={currentChat} layout={layout} room={room} canSplit={canSplit && count < MAX_PANES}
      onFocus={() => setLayout(l => focusLeaf(l, node.id))} onStartDrag={startPaneDrag}
      onZoom={() => { setLayout(l => focusLeaf(l, node.id)); setZoom(z => z === node.id ? null : node.id) }}
      onClose={() => { setZoom(null); onClosePane?.(node.id) }} onSplit={split} onDropInPane={onDropInPane} onPreview={setPreview} onClearPreview={clearPreview} />)}
    {!zoomId && geometry.dividers.map(d => <div key={d.id} data-divider-id={d.id} className={`split-divider split-divider--${d.dir}`}
      style={{ left: `${d.x * 100}%`, top: `${d.y * 100}%`, width: d.dir === 'row' ? 9 : `${d.w * 100}%`, height: d.dir === 'col' ? 9 : `${d.h * 100}%` }}
      role="separator" tabIndex={0} aria-label="Resize the panes" aria-orientation={d.dir === 'row' ? 'vertical' : 'horizontal'}
      aria-valuenow={Math.round((d.dir === 'row' ? (d.x - d.bounds.x) / d.bounds.w : (d.y - d.bounds.y) / d.bounds.h) * 100)} aria-valuemin={0} aria-valuemax={100}
      title="Drag to resize - double-click to equalize" onPointerDown={e => startResize(e, d)} onDoubleClick={() => setLayout(l => { const candidate = balanceSplit(l, d.splitId); return layoutFits(candidate, room.width, room.height) ? candidate : l })}
      onKeyDown={e => { const keys = d.dir === 'row' ? ['ArrowLeft', 'ArrowRight'] : ['ArrowUp', 'ArrowDown']; if (!keys.includes(e.key)) return; e.preventDefault(); setLayout(l => resizeWorkspaceDivider(l, d, (e.key === keys[0] ? -1 : 1) * (e.shiftKey ? .1 : .025), room.width, room.height)) }} />)}
    {preview?.rect && <div className="split-drop" style={position(preview.rect)} aria-hidden="true"><span>{preview.label}</span></div>}
    {preview?.targets && <div className="split-targets" style={{ left: `calc(${(preview.targets.x + preview.targets.w / 2) * 100}% - 65px)`, top: `calc(${(preview.targets.y + preview.targets.h / 2) * 100}% - 57px)` }} aria-hidden="true">
      {['top', 'left', 'center', 'right', 'bottom'].map(zone => <span key={zone} className={`split-target split-target--${zone}${zone === preview.zone ? ' on' : ''}`}>{({ top: '↑', left: '←', center: 'Swap', right: '→', bottom: '↓' })[zone]}</span>)}
    </div>}
    {dragging && dragLeaf && <div ref={ghostRef} className="split-drag-ghost" style={{ left: dragging.x, top: dragging.y }}>{labelFor(dragLeaf)}</div>}
    {layoutControl && <WorkspaceLayouts layout={layout} width={room.width} height={room.height} anchor={layoutControl} currentChat={currentChat}
      onPreset={preset => { setZoom(null); const candidate = presetLayout(latest.current, preset); if (layoutFits(candidate, room.width, room.height)) setLayout(candidate) }}
      onEqualize={() => setLayout(l => { const candidate = equalizeWorkspace(l); return layoutFits(candidate, room.width, room.height) ? candidate : l })} onSave={onSaveLayout} onRestore={onRestoreLayout} onClose={closePanel} />}
  </div>
}

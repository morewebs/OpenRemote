import { PANE_MIN_PX, MAX_PANES, closeLeaf, leafById, leaves, setLeafView } from './split.js'

export const PANE_MIN_HEIGHT = 180
export const WORKSPACE_PRESETS = ['columns', 'rows', 'main-stack', 'grid']

// HTML drag data is unreadable during hover; this app-local hint lets the
// preview distinguish moving a visible pane from opening another chat.
let sidebarDrag = null
export const sidebarDraggedChat = () => sidebarDrag
export const setSidebarDraggedChat = id => { sidebarDrag = id }

function replace(node, id, value) {
  if (node.id === id) return value
  if (node.type === 'leaf') return node
  const children = node.children.map(child => replace(child, id, value))
  return children.every((child, i) => child === node.children[i]) ? node : { ...node, children }
}

function without(node, id) {
  if (node.id === id) return null
  if (node.type === 'leaf') return node
  const pairs = node.children.map((child, i) => ({ child: without(child, id), size: node.sizes[i] })).filter(p => p.child)
  if (pairs.length === 1) return pairs[0].child
  const sum = pairs.reduce((total, p) => total + p.size, 0)
  return { ...node, children: pairs.map(p => p.child), sizes: pairs.map(p => p.size / sum) }
}

/** Move the actual leaf, keeping its composer scope and mounted identity. */
export function dockPane(layout, sourceId, targetId, zone) {
  const source = leafById(layout, sourceId)
  const target = leafById(layout, targetId)
  if (!source || !target || sourceId === targetId || !['center', 'left', 'right', 'top', 'bottom'].includes(zone)) return layout
  if (zone === 'center') {
    const swap = node => node.id === sourceId ? target : node.id === targetId ? source : node.type === 'leaf' ? node : { ...node, children: node.children.map(swap) }
    return { ...layout, root: swap(layout.root), focus: sourceId }
  }
  const seq = layout.seq + 1
  const before = zone === 'left' || zone === 'top'
  const root = replace(without(layout.root, sourceId), targetId, {
    type: 'split', id: `s${seq}`, dir: zone === 'left' || zone === 'right' ? 'row' : 'col',
    children: before ? [source, target] : [target, source], sizes: [.5, .5],
  })
  return { ...layout, root, seq, focus: sourceId }
}

/** Normalized rectangles keep pane DOM nodes independent of tree nesting. */
export function workspaceGeometry(layout) {
  const panes = []
  const dividers = []
  const walk = (node, bounds) => {
    if (node.type === 'leaf') {
      panes.push({ id: node.id, ...bounds })
      return
    }
    let offset = 0
    const row = node.dir === 'row'
    node.children.forEach((child, index) => {
      const share = node.sizes[index]
      walk(child, {
        x: bounds.x + (row ? offset * bounds.w : 0),
        y: bounds.y + (row ? 0 : offset * bounds.h),
        w: row ? share * bounds.w : bounds.w,
        h: row ? bounds.h : share * bounds.h,
      })
      offset += share
      if (index < node.children.length - 1) dividers.push({
        id: `${node.id}:${index}`, splitId: node.id, index, dir: node.dir, bounds,
        x: bounds.x + (row ? offset * bounds.w : 0),
        y: bounds.y + (row ? 0 : offset * bounds.h),
        w: row ? 0 : bounds.w, h: row ? bounds.h : 0,
      })
    })
  }
  walk(layout.root, { x: 0, y: 0, w: 1, h: 1 })
  return { panes, dividers }
}

export function layoutFits(layout, width, height) {
  return width > 0 && height > 0 && workspaceGeometry(layout).panes.every(p => p.w * width >= PANE_MIN_PX - .01 && p.h * height >= PANE_MIN_HEIGHT - .01)
}

/** A preset rearranges the existing working set, never discards a pane. */
export function presetLayout(layout, preset) {
  if (!WORKSPACE_PRESETS.includes(preset)) return layout
  const panes = leaves(layout.root)
  const count = Math.min(MAX_PANES, Math.max(panes.length, preset === 'grid' ? 4 : preset === 'main-stack' ? 3 : 2))
  let seq = layout.seq
  while (panes.length < count) panes.push({ type: 'leaf', id: `p${++seq}`, view: 'new' })
  const split = (dir, children, sizes = children.map(() => 1 / children.length)) => ({ type: 'split', id: `s${++seq}`, dir, children, sizes })
  let root
  if (preset === 'grid') root = split('row', [split('col', panes.slice(0, 2)), split('col', panes.slice(2))])
  else if (preset === 'main-stack') root = split('row', [panes[0], split('col', panes.slice(1))], [.6, .4])
  else root = split(preset === 'columns' ? 'row' : 'col', panes)
  return { ...layout, root, seq }
}

export function equalizeWorkspace(layout) {
  const walk = node => node.type === 'leaf' ? node : { ...node, sizes: node.children.map(() => 1 / node.children.length), children: node.children.map(walk) }
  return { ...layout, root: walk(layout.root) }
}

/** Closing the navigation owner promotes a survivor without changing its draft. */
export function closeWorkspacePane(layout, id, primaryView) {
  const ordered = leaves(layout.root)
  const target = leafById(layout, id)
  if (!target || ordered.length < 2) return { layout, primaryView }
  if (target.view !== null) return { layout: closeLeaf(layout, id), primaryView }
  const at = ordered.indexOf(target)
  const survivor = ordered[at + 1] ?? ordered[at - 1]
  const root = replace(without(layout.root, id), survivor.id, {
    ...survivor, view: null,
    scope: Object.hasOwn(survivor, 'scope') ? survivor.scope : survivor.id,
  })
  return { layout: { ...layout, root, focus: layout.focus === id ? survivor.id : layout.focus }, primaryView: survivor.view }
}

export function paneForView(layout, primaryView, view) {
  return leaves(layout.root).find(p => (p.view ?? primaryView) === view) ?? null
}

function minimumRoom(node) {
  if (node.type === 'leaf') return { width: PANE_MIN_PX, height: PANE_MIN_HEIGHT }
  const children = node.children.map(minimumRoom)
  return node.dir === 'row'
    ? { width: Math.max(...children.map((c, i) => c.width / node.sizes[i])), height: Math.max(...children.map(c => c.height)) }
    : { width: Math.max(...children.map(c => c.width)), height: Math.max(...children.map((c, i) => c.height / node.sizes[i])) }
}

/** Each side may contain more panes: reserve room for those descendants too. */
export function resizeWorkspaceDivider(layout, divider, delta, width, height) {
  if (!Number.isFinite(delta) || delta === 0) return layout
  const find = node => node.id === divider.splitId ? node : node.type === 'leaf' ? null : node.children.map(find).find(Boolean)
  const node = find(layout.root)
  if (!node || !node.children[divider.index + 1]) return layout
  const row = node.dir === 'row'
  const total = row ? width * divider.bounds.w : height * divider.bounds.h
  if (!(total > 0)) return layout
  const a = node.sizes[divider.index]
  const b = node.sizes[divider.index + 1]
  const minimumA = minimumRoom(node.children[divider.index])[row ? 'width' : 'height'] / total
  const minimumB = minimumRoom(node.children[divider.index + 1])[row ? 'width' : 'height'] / total
  if (a + b < minimumA + minimumB - .00001) return layout
  const moved = Math.max(minimumA - a, Math.min(b - minimumB, delta))
  if (moved === 0) return layout
  const sizes = [...node.sizes]
  sizes[divider.index] += moved
  sizes[divider.index + 1] -= moved
  return { ...layout, root: replace(layout.root, node.id, { ...node, sizes }) }
}

export function workspaceGestureValid(base, current, pointerId, expectedPointerId) {
  return pointerId === expectedPointerId && base.root === current.root && base.focus === current.focus
}

export function paneCompletion(layout, leafId, view) {
  const leaf = leafById(layout, leafId)
  return !leaf ? { target: 'closed', layout } : leaf.view === null
    ? { target: 'primary', layout }
    : { target: 'secondary', layout: setLeafView(layout, leafId, view) }
}

export function promotionMode(mode, record, thisDevice) {
  if (!record) return mode
  if (mode === 'cloud' && !record.executor) return 'local'
  if (mode === 'local' && record.executor && record.executor !== thisDevice) return 'cloud'
  return mode
}

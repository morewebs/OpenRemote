import test from 'node:test'
import assert from 'node:assert/strict'
import { leaves, newLayout, splitLeaf } from './split.js'
import * as workspace from './workspace-layout.js'

const pair = () => splitLeaf(newLayout(), 'p1', 'row', 'chat-b')
const four = () => splitLeaf(splitLeaf(pair(), 'p1', 'col', 'new'), 'p2', 'col', 'chat-d')
const ids = l => leaves(l.root).map(p => p.id)

test('docking moves the original pane and preserves identities at four panes', () => {
  const before = four()
  const snapshot = structuredClone(before)
  const after = workspace.dockPane(before, 'p1', 'p4', 'bottom')
  assert.equal(leaves(after.root).length, 4)
  assert.deepEqual(new Set(ids(after)), new Set(ids(before)))
  for (const pane of leaves(before.root)) assert.ok(leaves(after.root).includes(pane))
  assert.equal(after.focus, 'p1')
  assert.deepEqual(before, snapshot)
  const rectangles = workspace.workspaceGeometry(after).panes
  const original = rectangles.find(p => p.id === 'p1')
  const target = rectangles.find(p => p.id === 'p4')
  assert.equal(original.x, target.x)
  assert.equal(original.y, target.y + target.h)
})

test('center swaps positions without swapping draft scopes or views', () => {
  const before = pair()
  const after = workspace.dockPane(before, 'p1', 'p2', 'center')
  assert.deepEqual(ids(after), ['p2', 'p1'])
  assert.equal(leaves(after.root)[0], leaves(before.root)[1])
  assert.equal(leaves(after.root)[1].view, null)
  assert.deepEqual(after.root.sizes, before.root.sizes)
})

test('empty panes can dock, and invalid or self drops leave the exact layout untouched', () => {
  const before = splitLeaf(pair(), 'p2', 'col')
  assert.notEqual(workspace.dockPane(before, 'p3', 'p1', 'right'), before)
  for (const [source, target, zone] of [['p1', 'p1', 'left'], ['missing', 'p1', 'left'], ['p1', 'missing', 'left'], ['p1', 'p2', 'invalid']]) {
    assert.equal(workspace.dockPane(before, source, target, zone), before)
  }
})

test('a two-pane dock previews the final whole-workspace half, not a quarter', () => {
  const after = workspace.dockPane(pair(), 'p1', 'p2', 'bottom')
  const { panes } = workspace.workspaceGeometry(after)
  assert.deepEqual(panes.find(p => p.id === 'p1'), { id: 'p1', x: 0, y: .5, w: 1, h: .5 })
  assert.equal(after.root.dir, 'col')
})

test('geometry covers the workspace and exposes divider bounds for nested resizing', () => {
  const { panes, dividers } = workspace.workspaceGeometry(four())
  assert.equal(panes.reduce((sum, p) => sum + p.w * p.h, 0), 1)
  assert.equal(dividers.length, 3)
  assert.ok(dividers.every(d => ['row', 'col'].includes(d.dir) && d.bounds.w > 0 && d.bounds.h > 0))
  for (const p of panes) assert.ok(p.x >= 0 && p.y >= 0 && p.x + p.w <= 1 && p.y + p.h <= 1)
})

test('presets retain existing pane objects and create only missing slots', () => {
  const before = pair()
  const after = workspace.presetLayout(before, 'grid')
  assert.equal(leaves(after.root).length, 4)
  assert.equal(leaves(after.root)[0], leaves(before.root)[0])
  assert.equal(leaves(after.root)[1], leaves(before.root)[1])
  assert.ok(leaves(after.root).slice(2).every(p => p.view === 'new'))
  const rearranged = workspace.presetLayout(after, 'columns')
  assert.deepEqual(ids(rearranged), ids(after))
  assert.equal(rearranged.root.children.length, 4)
  assert.equal(workspace.presetLayout(before, 'unknown'), before)
})

test('closing the original promotes its neighbor with its draft scope and route', () => {
  const before = pair()
  const result = workspace.closeWorkspacePane(before, 'p1', 'chat-a')
  assert.equal(result.primaryView, 'chat-b')
  assert.equal(result.layout.root.id, 'p2')
  assert.equal(result.layout.root.view, null)
  assert.equal(result.layout.root.scope, 'p2')
  assert.equal(result.layout.focus, 'p2')
  assert.equal(workspace.closeWorkspacePane(result.layout, 'p2', result.primaryView).layout, result.layout)
})

test('promotion preserves a prior explicit composer scope, including null', () => {
  const before = pair()
  before.root.children[1] = { ...before.root.children[1], scope: null, view: 'new' }
  const result = workspace.closeWorkspacePane(before, 'p1', 'chat-a')
  assert.equal(result.primaryView, 'new')
  assert.equal(result.layout.root.scope, null)
})

test('closing secondary panes retains primary navigation and surviving focus', () => {
  const before = four()
  const result = workspace.closeWorkspacePane(before, 'p4', 'chat-a')
  assert.equal(result.primaryView, 'chat-a')
  assert.equal(leaves(result.layout.root).filter(p => p.view === null).length, 1)
  assert.ok(ids(result.layout).includes(result.layout.focus))
})

test('fit uses the actual workspace dimensions and both horizontal and vertical floors', () => {
  assert.equal(workspace.layoutFits(pair(), 480, 200), true)
  assert.equal(workspace.layoutFits(pair(), 479, 200), false)
  const grid = workspace.presetLayout(pair(), 'grid')
  assert.equal(workspace.layoutFits(grid, 700, 359), false)
  assert.equal(workspace.layoutFits(grid, 700, 400), true)
  assert.equal(workspace.layoutFits(grid, 0, 0), false)
})

test('equalizing reaches nested dividers and leaves pane state untouched', () => {
  const before = four()
  before.root.sizes = [.7, .3]
  before.root.children[0].sizes = [.8, .2]
  const after = workspace.equalizeWorkspace(before)
  assert.deepEqual(after.root.sizes, [.5, .5])
  assert.deepEqual(after.root.children[0].sizes, [.5, .5])
  assert.deepEqual(ids(after), ids(before))
})

test('already-visible chat lookup includes the navigation-owning pane', () => {
  const before = pair()
  assert.equal(workspace.paneForView(before, 'chat-a', 'chat-a').id, 'p1')
  assert.equal(workspace.paneForView(before, 'chat-a', 'chat-b').id, 'p2')
  assert.equal(workspace.paneForView(before, 'chat-a', 'missing'), null)
})

test('resizing respects the width of every nested pane, not only its group', () => {
  let layout = workspace.presetLayout(pair(), 'columns')
  layout = splitLeaf(layout, 'p1', 'col', 'new')
  const rootDivider = workspace.workspaceGeometry(layout).dividers.find(d => d.splitId === layout.root.id)
  const after = workspace.resizeWorkspaceDivider(layout, rootDivider, -.49, 800, 600)
  assert.equal(workspace.layoutFits(after, 800, 600), true)
  const rectangles = workspace.workspaceGeometry(after).panes
  assert.ok(rectangles.every(p => p.w * 800 >= 240 - .01))
})

test('vertical resizing uses its height floor and does not jump on cancel/no movement', () => {
  const layout = workspace.presetLayout(pair(), 'rows')
  const divider = workspace.workspaceGeometry(layout).dividers[0]
  const resized = workspace.resizeWorkspaceDivider(layout, divider, -.4, 700, 400)
  assert.equal(workspace.workspaceGeometry(resized).panes[0].h * 400, 180)
  assert.equal(workspace.resizeWorkspaceDivider(layout, divider, 0, 700, 400), layout)
  assert.equal(workspace.resizeWorkspaceDivider(layout, divider, .2, 700, 200), layout)
})

test('a fixed unequal nested share reserves enough room for its smallest child', () => {
  const layout = workspace.presetLayout(four(), 'columns')
  const panes = leaves(layout.root)
  layout.root = { type: 'split', id: 'outer', dir: 'row', sizes: [.75, .25], children: [
    { type: 'split', id: 'inner', dir: 'row', sizes: [.66, .34], children: panes.slice(0, 2) }, panes[2],
  ] }
  const divider = workspace.workspaceGeometry(layout).dividers.find(d => d.splitId === 'outer')
  assert.equal(workspace.layoutFits(layout, 1100, 600), true)
  const after = workspace.resizeWorkspaceDivider(layout, divider, -.35, 1100, 600)
  assert.equal(workspace.layoutFits(after, 1100, 600), true)
})

test('a stale or foreign-pointer gesture cannot overwrite a newer workspace', () => {
  const before = pair()
  assert.equal(workspace.workspaceGestureValid(before, before, 7, 7), true)
  assert.equal(workspace.workspaceGestureValid(before, before, 8, 7), false)
  assert.equal(workspace.workspaceGestureValid(before, { ...before, focus: 'p1' }, 7, 7), false)
  assert.equal(workspace.workspaceGestureValid(before, splitLeaf(before, 'p1', 'col'), 7, 7), false)
})

test('late creation resolves the originating leaf current role and ignores a closed leaf', () => {
  const before = pair()
  const promoted = workspace.closeWorkspacePane(before, 'p1', 'chat-a').layout
  assert.equal(workspace.paneCompletion(promoted, 'p2', 'created').target, 'primary')
  assert.equal(workspace.paneCompletion(promoted, 'p1', 'created').target, 'closed')
  const secondary = workspace.paneCompletion(before, 'p2', 'created')
  assert.equal(secondary.target, 'secondary')
  assert.equal(leaves(secondary.layout.root).find(p => p.id === 'p2').view, 'created')
})

test('promotion selects a mode that can display its surviving chat', () => {
  assert.equal(workspace.promotionMode('cloud', { executor: null }, 'here'), 'local')
  assert.equal(workspace.promotionMode('local', { executor: 'remote' }, 'here'), 'cloud')
  assert.equal(workspace.promotionMode('local', { executor: 'here' }, 'here'), 'local')
})

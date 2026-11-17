import test from 'node:test'
import assert from 'node:assert/strict'
import { orderProjects, moveProject, filterProjectRows, projectRows, loadProjectPreferences, storeProjectPreferences, collapseOtherProjects, pruneCollapsedProjects } from './project-workflow.js'

const projects = [
  { id: 'b', folders: ['/work/beta'] },
  { id: 'a', folders: ['/work/alpha'] },
  { id: 'c', folders: ['/other/empty'] },
]
const rows = [
  { id: 'one', title: 'Fix composer', workspace: '/work/alpha', updatedAt: 2 },
  { id: 'two', title: 'Navigation', workspace: '/work/beta', updatedAt: 8 },
  { id: 'three', title: 'Another task', workspace: '/work/alpha', updatedAt: 9, status: 'waiting' },
  { id: 'flat', title: 'Fix unrelated', workspace: null, updatedAt: 3 },
]
const ids = list => list.map(p => p.id)

test('project pins precede manual, name and recent ordering without mutating input', () => {
  assert.deepEqual(ids(orderProjects(projects, rows, { sort: 'name', pins: ['c'] })), ['c', 'a', 'b'])
  assert.deepEqual(ids(orderProjects(projects, rows, { sort: 'recent' })), ['a', 'b', 'c'])
  assert.deepEqual(ids(orderProjects(projects, rows, { order: ['c', 'a', 'missing'] })), ['c', 'a', 'b'])
  assert.deepEqual(ids(projects), ['b', 'a', 'c'])
})

test('movement follows the displayed order and never crosses the pinned boundary', () => {
  const prefs = { sort: 'name', pins: ['c'] }
  const moved = moveProject(orderProjects(projects, rows, prefs), 'b', -1, prefs)
  assert.equal(moved.sort, 'manual')
  assert.deepEqual(ids(orderProjects(projects, rows, moved)), ['c', 'b', 'a'])
  assert.deepEqual(moveProject(orderProjects(projects, rows, prefs), 'a', -1, prefs), prefs)
})

test('project name and folder searches keep their chats; title searches retain matching rows only', () => {
  assert.deepEqual(ids(filterProjectRows(projects, rows, 'alpha').rows), ['one', 'three'])
  assert.deepEqual(ids(filterProjectRows(projects, rows, '/work/beta').projects), ['b'])
  assert.deepEqual(ids(filterProjectRows(projects, rows, 'fix').rows), ['one', 'flat'])
  assert.deepEqual(ids(filterProjectRows(projects, rows, 'fix').projects), ['a'])
  assert.deepEqual(ids(filterProjectRows(projects, rows, 'empty').projects), ['c'])
  assert.equal(filterProjectRows(projects, rows, 'missing').projects.length, 0)
})

test('explicit project filter composes with text and full membership includes waiting chats', () => {
  assert.deepEqual(ids(filterProjectRows(projects, rows, '', 'a').rows), ['one', 'three'])
  assert.deepEqual(ids(filterProjectRows(projects, rows, 'navigation', 'a').rows), [])
  assert.deepEqual(ids(projectRows(projects[1], rows)), ['one', 'three'])
})

test('preferences tolerate corruption and storage denial, and survive empty loading lists', () => {
  const values = new Map()
  const storage = { getItem: k => values.get(k), setItem: (k, v) => values.set(k, v) }
  storeProjectPreferences({ pins: ['a', 'a', 4], order: ['c'], sort: 'recent' }, storage)
  const prefs = loadProjectPreferences(storage)
  assert.deepEqual(prefs, { pins: ['a'], order: ['c'], sort: 'recent' })
  orderProjects([], [], prefs)
  assert.deepEqual(loadProjectPreferences(storage), prefs)
  assert.deepEqual(loadProjectPreferences({ getItem() { throw Error() } }), { pins: [], order: [], sort: 'manual' })
  assert.doesNotThrow(() => storeProjectPreferences(prefs, { setItem() { throw Error() } }))
})

test('collapse others considers all projects, including ones outside a search', () => {
  assert.deepEqual(collapseOtherProjects(projects, 'a'), { b: true, c: true })
})

test('initial loading preserves collapsed projects; an authoritative list prunes removed ids', () => {
  const collapsed = { a: true, gone: true }
  assert.equal(pruneCollapsedProjects(collapsed, [], false), collapsed)
  assert.deepEqual(pruneCollapsedProjects(collapsed, projects, true), { a: true })
})

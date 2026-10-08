import { test } from 'node:test'
import assert from 'node:assert/strict'

import { projectName, projectFor, sections } from './projects.js'

const P1 = { id: 'p1', folders: ['/home/u/website'] }
const P2 = { id: 'p2', folders: ['/home/u/daemon'] }

test('a project is named by its first folder', () => {
  assert.equal(projectName(P1), 'website')
  assert.equal(projectName({ id: 'x', folders: [] }), 'project')
  assert.equal(projectName(null), 'project')
})

test('a chat belongs to the project whose folders hold its workspace', () => {
  assert.equal(projectFor([P1, P2], '/home/u/website')?.id, 'p1')
  assert.equal(projectFor([P1, P2], '/home/u/elsewhere'), null)
  // A project-less chat names no workspace at all.
  assert.equal(projectFor([P1, P2], null), null)
  assert.equal(projectFor([P1, P2], undefined), null)
})

test('sections splits chats into projects and one flat list', () => {
  const rows = [
    { id: 'a', workspace: '/home/u/website' },
    { id: 'b', workspace: null }, // project-less
    { id: 'c', workspace: '/home/u/daemon' },
    { id: 'd', workspace: '/home/u/unregistered' },
  ]
  const out = sections([P1, P2], rows)
  assert.deepEqual(
    out.projects.map((g) => [g.project.id, g.items.map((r) => r.id)]),
    [
      ['p1', ['a']],
      ['p2', ['c']],
    ],
  )
  assert.deepEqual(out.flat.map((r) => r.id), ['b', 'd'])
})

test('a project with no chats holds no group', () => {
  const out = sections([P1, P2], [{ id: 'a', workspace: '/home/u/website' }])
  assert.equal(out.projects.length, 1)
  assert.deepEqual(out.flat, [])
})

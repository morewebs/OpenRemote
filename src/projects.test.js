import { test } from 'node:test'
import assert from 'node:assert/strict'

import { heldStates, projectIndex, projectLine, projectName, projectFor, sections, unreadProjects, waitingProjects } from './projects.js'

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

test('a registered project with no chats is still listed, with its count', () => {
  const out = sections([P1, P2], [{ id: 'a', workspace: '/home/u/website' }])
  assert.deepEqual(
    out.projects.map((g) => [g.project.id, g.items.length]),
    [
      ['p1', 1],
      ['p2', 0],
    ],
    'registering a folder must not end with the project missing from the list',
  )
  assert.deepEqual(out.flat, [])
})

test('a project with no chats comes back even when nothing is running', () => {
  const out = sections([P1], [])
  assert.equal(out.projects.length, 1)
  assert.deepEqual(out.projects[0].items, [])
})

// ---- the grouping cost ----

const ROWS = Array.from({ length: 200 }, (_, i) => ({
  id: `c${i}`,
  workspace: i % 2 ? `/work/alpha` : `/work/beta`,
}))

test('sections through an index is one lookup per row, never a scan of every project', () => {
  const projects = [
    { id: 'p1', name: 'Alpha', folders: ['/work/alpha'] },
    { id: 'p2', name: 'Beta', folders: ['/work/beta'] },
  ]
  const index = projectIndex(projects)
  let lookups = 0
  const counting = { get: (key) => (lookups++, index.get(key)) }
  // Once the index exists, a project's folders are never read again: a getter
  // that throws proves the scan is gone.
  for (const project of projects) {
    Object.defineProperty(project, 'folders', {
      get() {
        throw new Error('scanned every project per row')
      },
    })
  }
  const grouped = sections(projects, ROWS, counting)
  assert.equal(lookups, ROWS.length, 'one lookup per row')
  assert.equal(grouped.projects.find((g) => g.project.id === 'p1').items.length, 100)
  assert.equal(grouped.projects.find((g) => g.project.id === 'p2').items.length, 100)
})

test('the index and the old scan group identically, first project winning', () => {
  const projects = [
    { id: 'first', name: 'First', folders: ['/work/shared'] },
    { id: 'second', name: 'Second', folders: ['/work/shared'] },
  ]
  const rows = [{ id: 'a', workspace: '/work/shared' }, { id: 'b', workspace: '/work/nowhere' }]
  assert.deepEqual(sections(projects, rows, projectIndex(projects)), sections(projects, rows))
  assert.deepEqual(projectFor(projects, '/work/shared').id, 'first')
  assert.deepEqual(projectIndex(projects).get('/work/shared').id, 'first')
})

test('unread counts are collected in one pass', () => {
  const projects = [
    { id: 'p1', name: 'Alpha', folders: ['/work/alpha'] },
    { id: 'p2', name: 'Beta', folders: ['/work/beta'] },
  ]
  const index = projectIndex(projects)
  const rows = [
    { id: 'c1', workspace: '/work/alpha' },
    { id: 'c2', workspace: '/work/beta' },
    { id: 'c3', workspace: '/work/nowhere' },
    { id: 'c4', workspace: '/work/alpha' },
  ]
  const unread = new Set(['c1', 'c2', 'c3'])
  assert.deepEqual([...unreadProjects(rows, unread, index)].sort(), ['p1', 'p2'])
  assert.equal(unreadProjects(rows, new Set(), index).size, 0)
})

// ---- what a project's head says ----

test('a project head states its own chats, and how many are waiting', () => {
  // A head with nothing to say says nothing: expanding the list is the answer
  // to "what is in here", and the waiting count is the only fact the list does
  // not show without reading every row's status.
  assert.equal(projectLine({ chats: 3, waiting: 0 }, { collapsed: false }), null)
  assert.deepEqual(projectLine({ chats: 3, waiting: 2 }, { collapsed: false }), [
    { kind: 'waiting', text: '2 waiting' },
  ])
  // Collapsed, nobody can see the chats: the tally answers the question the
  // collapsed row raises.
  assert.deepEqual(projectLine({ chats: 3, waiting: 0 }, { collapsed: true }), [
    { kind: 'chats', text: '3 chats' },
  ])
  assert.deepEqual(projectLine({ chats: 3, waiting: 1 }, { collapsed: true }), [
    { kind: 'chats', text: '3 chats' },
    { kind: 'waiting', text: '1 waiting' },
  ])
  assert.deepEqual(projectLine({ chats: 1 }, { collapsed: true }), [{ kind: 'chats', text: '1 chat' }])
  assert.deepEqual(projectLine({ chats: 0 }, { collapsed: true }), [{ kind: 'chats', text: '0 chats' }])
  assert.equal(projectLine({}, { collapsed: false }), null)
  assert.equal(projectLine(), null)
})

test('the head counts waiting with the row rule, not a second one', () => {
  const projects = [
    { id: 'p1', name: 'Alpha', folders: ['/work/alpha'] },
    { id: 'p2', name: 'Beta', folders: ['/work/beta'] },
  ]
  const index = projectIndex(projects)
  // The waiting chats the sidebar draws in the "Needs you" group, which is why
  // they are not in the grouped items at all.
  const waiting = [
    // Waiting by its live status, whatever the poll says: the row's amber.
    { id: 'a', workspace: '/work/alpha', liveStatus: 'waiting', polledStatus: 'idle' },
    // Waiting only by the poll.
    { id: 'b', workspace: '/work/alpha', polledStatus: 'waiting' },
    // Busy is not waiting.
    { id: 'c', workspace: '/work/alpha', liveStatus: 'working', polledStatus: 'waiting' },
    // The live status wins both ways: idle now, though the poll still says waiting.
    { id: 'd', workspace: '/work/alpha', liveStatus: 'idle', polledStatus: 'waiting' },
    // Another project's chat.
    { id: 'e', workspace: '/work/beta', liveStatus: 'waiting' },
    // A chat outside every project names no workspace the index knows.
    { id: 'f', workspace: null, liveStatus: 'waiting' },
  ]
  const counts = waitingProjects(waiting, index)
  assert.equal(counts.get('p1'), 2, 'only a and b are waiting by the rule')
  assert.equal(counts.get('p2'), 1, 'e is beta\'s, and the count lives per project')
  assert.equal(counts.size, 2, 'the project-less waiting chat belongs to no head')
  // The grouping is untouched by the tally: a waiting chat is not in `items`,
  // because the group above already draws it.
  const out = sections(projects, [{ id: 'g', workspace: '/work/alpha' }], index)
  assert.equal(out.projects[0].items.length, 1)
  assert.equal(out.projects[0].waiting, undefined, 'sections does not count waiting; the head does')
  // The tally is one pass, and the same row can only be counted once.
  assert.deepEqual([...waitingProjects([waiting[0], waiting[0]], index).values()], [2])
  assert.equal(waitingProjects(undefined, index).size, 0)
  assert.equal(waitingProjects(waiting, null).size, 0)
})
// ---- a folded project still says what runs and what failed inside it ----

test('a folded head rolls up its failed and running chats; an open one leaves them to the rows', () => {
  // Folded, nothing inside can be seen, so a failure or a running turn would
  // hide behind the fold. The head says both, after the tally and the waiting
  // count, and says nothing of a state no chat is in.
  assert.deepEqual(projectLine({ chats: 4, waiting: 1, failed: 1, running: 2 }, { collapsed: true }), [
    { kind: 'chats', text: '4 chats' },
    { kind: 'waiting', text: '1 waiting' },
    { kind: 'failed', text: '1 failed' },
    { kind: 'running', text: '2 running' },
  ])
  assert.deepEqual(projectLine({ chats: 2, failed: 0, running: 1 }, { collapsed: true }), [
    { kind: 'chats', text: '2 chats' },
    { kind: 'running', text: '1 running' },
  ])
  // Open, the rows are on screen: the head keeps to what it said before.
  assert.equal(projectLine({ chats: 4, failed: 1, running: 2 }, { collapsed: false }), null)
})

test('the rollup counts failed and running chats in either vocabulary, and never an away copy', () => {
  const rows = [
    { status: 'failed' },
    { status: 'running' },
    { status: 'working' },
    { status: 'starting' },
    { status: 'idle' },
    { status: 'stopped' },
    // Waiting chats sit in the group above and are counted there.
    { status: 'waiting' },
    // A copy whose device is away has no trustworthy status.
    { status: 'failed', offline: true },
    null,
  ]
  assert.deepEqual(heldStates(rows), { failed: 1, running: 3 })
  assert.deepEqual(heldStates([]), { failed: 0, running: 0 })
  assert.deepEqual(heldStates(), { failed: 0, running: 0 })
})

// Moving a chat between projects: the rules every surface shares - the
// sidebar's picker and its drop, the pane's menu, the confirm.

import assert from 'node:assert/strict'
import test from 'node:test'

const store = new Map()
globalThis.localStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k),
  clear: () => store.clear(),
}

const { askBeforeMove, busyReason, dropMoves, moveSentence, moveTargets, setAskBeforeMove, unreachableNote, wouldMove } = await import(
  './move-chat.js'
)

const projects = [
  { id: 'p1', folders: ['/dev/webapp'] },
  { id: 'p2', folders: ['/dev/api'] },
]

test('a chat can go to No project first, then its machine\'s projects, the current one marked', () => {
  const targets = moveTargets(projects, '/dev/webapp')
  assert.deepEqual(
    targets.map((t) => [t.project?.id ?? null, t.name, t.current]),
    [
      [null, 'No project', false],
      ['p1', 'webapp', true],
      ['p2', 'api', false],
    ],
  )
  // A folder no project holds is already in No project.
  assert.equal(moveTargets(projects, 'C:\elsewhere')[0].current, true)
  assert.equal(moveTargets(projects, null)[0].current, true)
  assert.deepEqual(moveTargets(null, null).map((t) => t.name), ['No project'])
})

test('a drop moves a chat only where it is not already', () => {
  assert.equal(wouldMove(projects, '/dev/webapp', projects[0]), false)
  assert.equal(wouldMove(projects, '/dev/webapp', projects[1]), true)
  // The Chats section is "No project".
  assert.equal(wouldMove(projects, '/dev/webapp', null), true)
  assert.equal(wouldMove(projects, 'C:\elsewhere', null), false)
  assert.equal(wouldMove(projects, null, projects[1]), true)
})

test('a running turn keeps a chat where it is; starting does not', () => {
  assert.match(busyReason('running'), /turn is running/)
  assert.match(busyReason('waiting'), /turn is running/)
  // A harness that names its conversation at the first message shows
  // "starting" until then, and a chat can move again right after a move.
  assert.equal(busyReason('starting'), null)
  assert.equal(busyReason('idle'), null)
  assert.equal(busyReason('stopped'), null)
})

test('the confirm says what changes: the folder the agent works in next', () => {
  assert.equal(
    moveSentence({ chatTitle: 'Fix login', to: projects[1] }),
    'From its next message the agent in "Fix login" works in /dev/api.',
  )
  assert.equal(
    moveSentence({ chatTitle: 'Fix login', to: null }),
    'From its next message the agent in "Fix login" works in the home folder.',
  )
})

test('asking first is this device\'s choice, on until turned off', () => {
  localStorage.clear()
  assert.equal(askBeforeMove(), true)
  setAskBeforeMove(false)
  assert.equal(askBeforeMove(), false)
  setAskBeforeMove(true)
  assert.equal(askBeforeMove(), true)
})

test('a chat whose machine can\'t be asked says why instead of offering projects', () => {
  assert.equal(unreachableNote('offline', 'box'), 'box is offline')
  assert.equal(unreachableNote('not-machine', 'box'), "box isn't a machine")
  assert.equal(unreachableNote('signed-out', 'box'), 'Sign in to Cloud first')
  assert.equal(unreachableNote('gone', 'box'), 'box left your Cloud')
  assert.equal(unreachableNote('unknown', 'box'), "Couldn't reach box")
  // Here and a reachable machine have projects to show, not a reason.
  assert.equal(unreachableNote('here', 'box'), null)
  assert.equal(unreachableNote('remote', 'box'), null)
})

test('a drop moves only a chat this computer runs, onto this computer\'s projects', () => {
  const row = { workspace: '/dev/webapp', status: 'idle' }
  assert.equal(dropMoves(projects, row, projects[1], 'here'), true)
  assert.equal(dropMoves(projects, row, projects[0], 'here'), false, 'already there')
  // The sidebar's projects are this computer's: a chat on another machine
  // moves through its menu, which asks that machine for its own.
  assert.equal(dropMoves(projects, row, projects[1], 'remote'), false)
  assert.equal(dropMoves(projects, null, projects[1], 'here'), false)
})

// A chat's place in git, as the header and the sidebar say it, and the
// shared cache that asks the daemon for many folders at once.

import assert from 'node:assert/strict'
import test from 'node:test'
import { placeStore, placeWords } from './git-places.js'

test('a place reads as its branch, and a linked worktree names its repository', () => {
  assert.equal(placeWords(null), null)
  assert.deepEqual(placeWords({ branch: 'main', worktree: false, root: '/r', main: '/r' }), {
    branch: 'main',
    worktree: false,
    of: null,
    title: 'Branch main',
  })
  const linked = placeWords({ branch: 'feat/login', worktree: true, root: String.raw`C:\code\site.feat`, main: String.raw`C:\code\site` })
  assert.equal(linked.branch, 'feat/login')
  assert.equal(linked.of, 'site')
  assert.ok(linked.title.endsWith(String.raw`worktree of C:\code\site`), linked.title)
  assert.equal(placeWords({ branch: 'x', worktree: true, main: '/home/me/code/site/' }).of, 'site')
  assert.equal(placeWords({ branch: null, detached: '0352a86', worktree: false, main: '/r' }).branch, 'detached at 0352a86')
  assert.equal(placeWords({ branch: null, detached: null }), null)
})

test('folders asked for together go in one request, and a change tells the listeners once', async () => {
  const store = placeStore()
  const asked = []
  const api = {
    async gitPlaces(paths) {
      asked.push(paths)
      return { places: Object.fromEntries(paths.map((p) => [p, p === '/none' ? null : { branch: 'b', worktree: true, main: '/m' }])) }
    },
  }
  let heard = 0
  store.subscribe(() => heard++)
  store.ask(api, ['/a', '/b'])
  await store.ask(api, ['/c', '/none'])
  assert.deepEqual(asked, [['/a', '/b', '/c', '/none']])
  assert.equal(heard, 1)
  assert.equal(store.get('/a').branch, 'b')
  assert.equal(store.get('/none'), null)
  // The same answer again changes nothing.
  await store.ask(api, ['/a'])
  assert.equal(heard, 1)
  // An older daemon without the route: nothing shown, nothing thrown.
  const old = placeStore()
  await old.ask({ async gitPlaces() { throw new Error('404') } }, ['/x'])
  assert.equal(old.get('/x'), null)
  await old.ask({}, ['/y'])
})

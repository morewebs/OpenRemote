import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

// settings.js touches localStorage at import time? No - only inside its
// functions - but the storage itself must exist before any call.
const store = new Map()
globalThis.localStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => store.set(k, String(v)),
  removeItem: (k) => store.delete(k),
  clear: () => store.clear(),
}

const {
  loadPrefs,
  savePrefs,
  applyReduceMotion,
  applyDensity,
  applyPrivacyMode,
  restoreBootPrefs,
  hashWriteReady,
  loadRecentWorkspaces,
  pushRecentWorkspace,
  samePath,
  loadStoredChatSeen,
  saveStoredChatSeen,
} = await import('./settings.js')

const stubDoc = () => {
  const attrs = new Map()
  return {
    attrs,
    documentElement: {
      setAttribute: (n, v) => attrs.set(n, v),
      removeAttribute: (n) => attrs.delete(n),
    },
  }
}

test('prefs round-trip through the single key', () => {
  localStorage.clear()
  savePrefs({ reduce_motion: true, density: 'compact', startup_view: 'last' })
  const p = loadPrefs()
  assert.equal(p.reduce_motion, true)
  assert.equal(p.density, 'compact')
  assert.equal(p.startup_view, 'last')
})

test('the old reduce-motion key migrates when the new key is absent', () => {
  localStorage.clear()
  localStorage.setItem('openremote-reduce-motion', '1')
  assert.equal(loadPrefs().reduce_motion, true)
  // And a save writes the new key, so the old one never matters again.
  savePrefs({ density: 'compact' })
  localStorage.removeItem('openremote-reduce-motion')
  assert.equal(loadPrefs().reduce_motion, true)
})

test('defaults fall back when storage holds garbage', () => {
  localStorage.clear()
  localStorage.setItem('openremote-prefs', '{not json')
  const p = loadPrefs()
  assert.equal(p.reduce_motion, false)
  assert.equal(p.density, 'comfortable')
  assert.equal(p.startup_view, 'new')
  // An unknown density value is dropped rather than trusted.
  localStorage.setItem('openremote-prefs', JSON.stringify({ density: 'cozy' }))
  assert.equal(loadPrefs().density, 'comfortable')
})

test('apply functions set and clear the html attributes', () => {
  const doc = stubDoc()
  applyReduceMotion(doc, true)
  assert.ok(doc.attrs.has('data-reduce-motion'))
  applyReduceMotion(doc, false)
  assert.ok(!doc.attrs.has('data-reduce-motion'))
  applyDensity(doc, 'compact')
  assert.equal(doc.attrs.get('data-density'), 'compact')
})

test('restoreBootPrefs applies the persisted look before first paint', () => {
  localStorage.clear()
  savePrefs({ reduce_motion: true, density: 'compact' })
  const doc = stubDoc()
  const loc = { hash: '' }
  restoreBootPrefs(doc, loc)
  assert.ok(doc.attrs.has('data-reduce-motion'))
  assert.equal(doc.attrs.get('data-density'), 'compact')
  // startup_view defaults to 'new' - the hash stays untouched.
  assert.equal(loc.hash, '')
})

test('privacy mode is off unless saved as exactly true, and survives a restart', () => {
  localStorage.clear()
  assert.equal(loadPrefs().privacy_mode, false)
  // Only a real boolean turns it on - a stray truthy value never blurs
  // the app by accident.
  localStorage.setItem('openremote-prefs', JSON.stringify({ privacy_mode: 'yes' }))
  assert.equal(loadPrefs().privacy_mode, false)
  localStorage.clear()
  savePrefs({ privacy_mode: true })
  // Saving another pref keeps it.
  savePrefs({ density: 'compact' })
  assert.equal(loadPrefs().privacy_mode, true)
  // Restored before first paint, so a stream that starts private never
  // shows one readable frame.
  const doc = stubDoc()
  restoreBootPrefs(doc, { hash: '' })
  assert.ok(doc.attrs.has('data-privacy'))
})

test('applyPrivacyMode sets and clears its html attribute', () => {
  const doc = stubDoc()
  applyPrivacyMode(doc, true)
  assert.ok(doc.attrs.has('data-privacy'))
  applyPrivacyMode(doc, false)
  assert.ok(!doc.attrs.has('data-privacy'))
})

test('restoreBootPrefs seeds the hash only when asked and only when empty', () => {
  localStorage.clear()
  savePrefs({ startup_view: 'last' })
  localStorage.setItem('openremote-last-view', 'plugins')
  const doc = stubDoc()
  const loc = { hash: '' }
  restoreBootPrefs(doc, loc)
  assert.equal(loc.hash, '#plugins')

  // A hash the user deep-linked to wins over the pref.
  const loc2 = { hash: '#automations' }
  restoreBootPrefs(doc, loc2)
  assert.equal(loc2.hash, '#automations')

  // Pref 'new' never seeds.
  savePrefs({ startup_view: 'new' })
  const loc3 = { hash: '' }
  restoreBootPrefs(doc, loc3)
  assert.equal(loc3.hash, '')

  // A garbage last-view is left for the router's known-set, not a crash.
  savePrefs({ startup_view: 'last' })
  localStorage.setItem('openremote-last-view', 'nonsense-view')
  const loc4 = { hash: '' }
  restoreBootPrefs(doc, loc4)
  assert.equal(loc4.hash, '#nonsense-view')
})

test('hashWriteReady holds while a chat seed waits for its first session beat', () => {
  // The boot the fix exists for: a chat id is seeded, no poll has
  // answered yet - the write effect must not clobber the hash.
  assert.equal(hashWriteReady({ sessionsLoaded: false, seedConsumed: false }), false)

  // The first beat answers (even an empty list): the reconcile consumes
  // the seed on that beat, so the write effect may run again.
  assert.equal(hashWriteReady({ sessionsLoaded: true, seedConsumed: false }), true)

  // No seed pending - a static seed validates at mount, nothing was
  // planted, or the user navigated past it: immediate, as before the fix.
  assert.equal(hashWriteReady({ sessionsLoaded: false, seedConsumed: true }), true)
  assert.equal(hashWriteReady({ sessionsLoaded: true, seedConsumed: true }), true)

  // Bare defaults are the closed hold.
  assert.equal(hashWriteReady(), false)
})

test('recents parse garbage, filter non-strings, and cap at eight', () => {
  localStorage.clear()
  localStorage.setItem('openremote-recent-workspaces', '{oops')
  assert.deepEqual(loadRecentWorkspaces(), [])
  localStorage.setItem('openremote-recent-workspaces', JSON.stringify(['a', 3, null, 'b']))
  assert.deepEqual(loadRecentWorkspaces(), ['a', 'b'])
})

test('pushRecentWorkspace dedupes, fronts, and caps at eight', () => {
  localStorage.clear()
  pushRecentWorkspace('w1')
  pushRecentWorkspace('w2')
  pushRecentWorkspace('w1')
  assert.deepEqual(loadRecentWorkspaces(), ['w1', 'w2'])
  for (let i = 0; i < 10; i++) pushRecentWorkspace(`extra-${i}`)
  assert.equal(loadRecentWorkspaces().length, 8)
  assert.equal(loadRecentWorkspaces()[0], 'extra-9')
})

test('two spellings of one folder are one recent', () => {
  localStorage.clear()
  pushRecentWorkspace('C:\\work')
  pushRecentWorkspace('c:\\work\\')
  assert.deepEqual(
    loadRecentWorkspaces(),
    ['c:\\work\\'],
    'the folder appears once, under the spelling used last',
  )
  // A POSIX path is case-sensitive, so these two really are two folders.
  pushRecentWorkspace('~/Work')
  pushRecentWorkspace('~/work')
  assert.equal(loadRecentWorkspaces().length, 3)
})

test('samePath folds a trailing separator and Windows case, nothing else', () => {
  assert.equal(samePath('C:\\work\\', 'c:\\work'), true)
  assert.equal(samePath('/home/me/proj/', '/home/me/proj'), true)
  assert.equal(samePath('~/Work', '~/work'), false, 'POSIX case is a different folder')
  assert.equal(samePath('a', 'b'), false)
})

test('a storage that refuses to answer leaves the defaults, before first paint', () => {
  // The boot reads prefs at module scope (main.jsx) so the first frame is
  // never animated, spaced or readable by accident. Every read in here
  // has to fall through to the defaults when storage refuses: one that
  // sat outside its guard took the whole boot down instead.
  const real = globalThis.localStorage
  globalThis.localStorage = {
    getItem: () => {
      throw new Error('storage is not available')
    },
    setItem: () => {
      throw new Error('storage is not available')
    },
    removeItem: () => {
      throw new Error('storage is not available')
    },
    clear: () => {},
  }
  try {
    const p = loadPrefs()
    assert.equal(p.reduce_motion, false)
    assert.equal(p.density, 'comfortable')
    assert.equal(p.startup_view, 'new')
    assert.equal(p.privacy_mode, false)
    // Saving under the same storage is already a no-op, never a throw.
    savePrefs({ density: 'compact' })
  } finally {
    globalThis.localStorage = real
  }
})

// ---- when this console last looked at each chat ----

test('chat records round-trip through their own key', () => {
  localStorage.clear()
  saveStoredChatSeen({ 'chat-1': 1700000000000, 'chat-2': 1700000000500 })
  assert.deepEqual(loadStoredChatSeen(), { 'chat-1': 1700000000000, 'chat-2': 1700000000500 })
  assert.ok(localStorage.getItem('openremote-chat-seen'))
})

test('a damaged chat record reads as no record at all', () => {
  for (const raw of ['{not json', '["a"]', '"a string"', '42', 'null']) {
    localStorage.clear()
    localStorage.setItem('openremote-chat-seen', raw)
    assert.deepEqual(loadStoredChatSeen(), {}, raw)
  }
})

test('a storage that refuses to answer leaves no records rather than throwing', () => {
  const original = globalThis.localStorage
  globalThis.localStorage = {
    getItem: () => {
      throw new Error('denied')
    },
    setItem: () => {
      throw new Error('denied')
    },
    removeItem: () => {},
    clear: () => {},
  }
  try {
    assert.deepEqual(loadStoredChatSeen(), {})
    saveStoredChatSeen({ a: 1 })
  } finally {
    globalThis.localStorage = original
  }
})

test('a preference the storage refused says it did not land, and keeps the old value', () => {
  localStorage.clear()
  // A write that works answers true, so the caller may show it as kept.
  assert.equal(savePrefs({ density: 'compact' }), true)
  assert.equal(loadPrefs().density, 'compact')
  // A storage that refuses the write: the answer is false, and what is stored
  // is the value from before, which is what the rows will re-read.
  const writable = globalThis.localStorage.setItem
  globalThis.localStorage.setItem = () => {
    throw new Error('denied')
  }
  try {
    assert.equal(savePrefs({ density: 'comfortable' }), false)
    assert.equal(savePrefs({ reduce_motion: true }), false)
  } finally {
    globalThis.localStorage.setItem = writable
  }
  assert.equal(loadPrefs().density, 'compact', 'the refused write changed nothing')
  assert.equal(loadPrefs().reduce_motion, false)
  // A storage that refuses both ways: no answer about what is stored, and no
  // write either, so the caller is told nothing landed.
  const readable = globalThis.localStorage.getItem
  const writable2 = globalThis.localStorage.setItem
  globalThis.localStorage.getItem = () => {
    throw new Error('denied')
  }
  globalThis.localStorage.setItem = () => {
    throw new Error('denied')
  }
  try {
    assert.equal(savePrefs({ density: 'comfortable' }), false)
  } finally {
    globalThis.localStorage.getItem = readable
    globalThis.localStorage.setItem = writable2
  }
  assert.equal(loadPrefs().density, 'compact')
})

// The view's side of the same contract: a write that did not land has to be
// said, and a document attribute may not move ahead of the stored preference.
// Read as source, the way the other views' wiring is pinned.
const viewSource = readFileSync(new URL('./SettingsView.jsx', import.meta.url), 'utf8')

test('the startup rows and the preference toggles say when a write did not land', () => {
  // Every startup catch speaks, instead of the comments that used to sit there.
  const said = viewSource.match(/setStartupNote\(errorCopy\(err\)\)/g) ?? []
  assert.equal(said.length, 3, 'Launch at login, the tray and the z-order row all report a refusal')
  assert.doesNotMatch(viewSource, /\{\s*\/\* the fact stays what it was \*\/\s*\}/)
  // A startup call that answers "still what it was" is surfaced too.
  assert.match(viewSource, /if \(enabled !== wanted\) \{/)
  assert.match(viewSource, /if \(next && typeof next\.keep_running === 'boolean' && next\.keep_running !== wanted\) \{/)
  assert.match(viewSource, /if \(next && typeof next\.keep_above === 'boolean' && next\.keep_above !== wanted\) \{/)
  // The preference write's answer is returned, and a failed one sets the line.
  assert.match(viewSource, /const landed = savePrefs\(partial\)/)
  assert.match(
    viewSource,
    /setStartupNote\(landed \? null : 'The console could not save that setting, so it is unchanged\.'\)/,
  )
  assert.match(viewSource, /return landed/)
  // The document follows a kept preference only, in all three places that set one.
  for (const [attribute, call] of [
    ['reduce motion', 'applyReduceMotion'],
    ['privacy mode', 'applyPrivacyMode'],
    ['density', 'applyDensity'],
  ]) {
    const at = viewSource.indexOf(`${call}(document, value)`)
    assert.ok(at > 0, `${attribute} applies its attribute`)
    const before = viewSource.slice(Math.max(0, at - 260), at)
    assert.match(before, /if \(!setPref\(/, `${attribute} applies it only when the save landed`)
  }
  // One line, drawn where the press happened: the Appearance, startup and
  // Confirmations rows each show it from the same piece of state.
  const lines = viewSource.match(/\{startupNote && \(\s*\n\s*<p className="pn-error" role="status">/g) ?? []
  assert.equal(lines.length, 3, 'the note renders under Appearance, Startup and Confirmations')
})
test('a stored set that cannot be read is not an empty set', () => {
  // The failure this guards: savePrefs used to build on loadPrefs, which answers
  // defaults for anything it cannot read. A profile that refused the read but
  // allowed the write would then write every other preference back as its
  // default, a silent reset of settings nobody touched.
  localStorage.clear()
  localStorage.setItem('openremote-prefs', '{not json')
  assert.equal(savePrefs({ density: 'compact' }), false, 'a set that cannot be read is refused')
  assert.equal(localStorage.getItem('openremote-prefs'), '{not json', 'and nothing was written over it')
  // An array is not a preference set either.
  localStorage.setItem('openremote-prefs', '[1,2]')
  assert.equal(savePrefs({ density: 'compact' }), false)
  // And the migration still lands on the first save: nothing stored means the
  // defaults plus the superseded key are the best truth there is.
  localStorage.clear()
  localStorage.setItem('openremote-reduce-motion', '1')
  assert.equal(savePrefs({ density: 'compact' }), true)
  assert.equal(loadPrefs().reduce_motion, true, 'the first save keeps the migrated value')
})
test('moving a chat is asked first until the dialog or Settings says otherwise', () => {
  localStorage.clear()
  assert.equal(loadPrefs().ask_move_chat, true, 'on by default')
  savePrefs({ ask_move_chat: false })
  assert.equal(loadPrefs().ask_move_chat, false)
  savePrefs({ ask_move_chat: true })
  assert.equal(loadPrefs().ask_move_chat, true)
  // Anything but an explicit false keeps asking.
  localStorage.setItem('openremote-prefs', JSON.stringify({ ask_move_chat: 'no' }))
  assert.equal(loadPrefs().ask_move_chat, true)
})

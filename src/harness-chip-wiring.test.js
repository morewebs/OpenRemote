// Selection opens the App picker; management remains an explicit footer
// action. Selecting another App prepares a new composer because the daemon
// binds an existing chat to the App that created it.
//
// Source text, like the repo's other wiring checks: this is the half a pure
// test cannot see, and a call site that loses its import is still valid text.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('App selection opens a working picker and management is an explicit action', () => {
  const view = read('ChatView.jsx')
  assert.match(view, /onClick=\{togglePicker\('harness'\)\}/)
  assert.match(view, /aria-expanded=\{picker\?\.kind === 'harness'\}/)
  assert.match(view, /kind === 'harness' && <PickerMenu label="App"/)
  assert.match(view, /onChooseApp\(\{ seed, composerKey \}\)/)
  assert.match(view, /onSelect: togglePicker\('harness'\)/, 'compact options use the same selector')
  assert.match(view, /setManagingHarnesses\(true\)[^\n]*>Manage apps…<\/button>/)
  assert.match(view, /\{managingHarnesses && !web && createPortal\(/)
  assert.match(view, /<HarnessManagerModal/)
})

// The wiring checks read source text, and source text does not care whether a
// name is bound. This one did not: the chip's title called `harnessName` while
// the view imported only `harnessLabel`, so the suite stayed green and the app
// threw `ReferenceError: harnessName is not defined` the moment a chat with a
// harness rendered. Every harness-names helper a view calls has to be imported
// by that view.
test('a view imports every harness-names helper it calls', () => {
  const view = read('ChatView.jsx')
  const imported = view.match(/import \{([^}]*)\} from '\.\/harness-names\.js'/)
  assert.ok(imported, 'ChatView.jsx imports from harness-names.js')
  const bound = imported[1].split(',').map((name) => name.trim())
  for (const helper of ['harnessLabel', 'harnessName']) {
    const called = new RegExp(`\\b${helper}\\s*\\(`).test(view)
    if (called) {
      assert.ok(
        bound.includes(helper),
        `ChatView.jsx calls ${helper}() but does not import it - that is a ReferenceError at render time`,
      )
    }
  }
})

test('the existing session stays bound to its App; a different App uses a new composer', () => {
  // The wire, read from the daemon: a chat's settings are the model, the
  // effort, fast and the permission word, and a harness is chosen when the
  // session starts. If this ever grows a harness field, a picker becomes
  // possible again, and this test is the place that says so.
  const http = readFileSync(
    new URL('../daemon/crates/openremote-daemon/src/http.rs', import.meta.url),
    'utf8',
  )
  const body = http.match(/struct SettingsBody \{[\s\S]*?\n\}/)
  assert.ok(body, 'the settings body is declared')
  assert.doesNotMatch(body[0], /harness/)
  assert.match(body[0], /request_id: String/)
  assert.match(body[0], /permission_mode: Option<String>/)
})

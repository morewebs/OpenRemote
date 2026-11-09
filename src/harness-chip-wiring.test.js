// The harness chip in a chat's composer used to open a listbox of every
// installed harness, and choosing a row only closed the listbox: a control
// that read as "switch this chat's harness" while switching nothing. The
// daemon takes a harness when it starts a session - a chat's settings are
// the model, the effort, fast, and the permission word - so the listbox
// could not have worked. The chip opens the harness manager instead, which
// is the one thing beside it that acts, and on a phone the only route to it,
// since the picker it replaced had no footer there.
//
// Source text, like the repo's other wiring checks: this is the half a pure
// test cannot see, and a call site that loses its import is still valid text.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the chat harness chip opens the manager, not a listbox that chose nothing', () => {
  const view = read('ChatView.jsx')
  // No harness listbox, and nothing left that would build one.
  assert.doesNotMatch(view, /kind === 'harness'/)
  assert.doesNotMatch(view, /togglePicker\('harness'/)
  assert.doesNotMatch(view, /import \{ pickable \}/)
  assert.doesNotMatch(view, /filter\(pickable\)/)
  // The chip itself closes any open picker and opens the manager.
  assert.match(
    view,
    /onClick=\{\(\) => \{\s*\n\s*hidePicker\(\)\s*\n\s*setManagingHarnesses\(true\)\s*\n\s*\}\}/,
  )
  // It announces a dialog rather than a listbox, and its title names the mark
  // before the action, so the harness stays the fact the chip shows. The name
  // is the one the chip prints: `harnessText` already resolved it, which also
  // keeps a plugin-provided harness reading as its own name rather than a bare
  // lowercase id.
  assert.match(view, /aria-haspopup="dialog"\s*\n\s*title=\{`App - \$\{harnessText\}/)
  assert.doesNotMatch(view, /aria-expanded=\{picker\?\.kind === 'harness'\}/)
  // The manager it opens is still rendered by this view.
  assert.match(view, /\{managingHarnesses && !web && \(/)
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

test('the daemon cannot take a harness on a running session, which is why the listbox went', () => {
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

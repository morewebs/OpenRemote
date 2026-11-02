// A Freebuff model row carries the harness's own two extra facts: the cost
// tier as a second line ("3x · Smart & Fast" under "DeepSeek V4.1 Flash 10/17
// • high") and a hover sentence. The New chat model menu also says how old the
// captured catalog is and offers to capture it again, and a harness whose
// settings are live can change its model while its chat is up. This suite has
// no renderer (the picker tests pin their wiring from the source for that
// reason), so the expressions that make each behaviour possible are what is
// pinned here; relative-time.test.js holds the words themselves.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { relativeTime } from './relative-time.js'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('a model row shows its cost tier as a second line', () => {
  const picker = source('PickerMenu.jsx')
  const css = source('pickermenu.css')
  // The catalog's words reach the row untouched: the daemon sends
  // "DeepSeek V4.1 Flash 10/17 • high" with "3x · Smart & Fast" as its detail,
  // and the detail is rendered as its own span after the name.
  const main = picker.slice(
    picker.indexOf('className="nc-item-main"'),
    picker.indexOf('renderSubline?.'),
  )
  const name = main.indexOf('item.name')
  const detail = main.indexOf('<span className="nc-item-detail">{item.detail}</span>')
  assert.ok(name > 0, 'the name is in the row main line')
  assert.ok(detail > name, 'and the detail is under it, after the name')
  // The harness's own sentence is the row's hover title, and a catalog that
  // carries none sets no title at all: never an empty tooltip.
  assert.match(picker, /title=\{item\.note \|\| undefined\}/)
  // Rows with a detail wrap so the tier can take the full width under the
  // name, and rows without one keep their single line.
  assert.match(picker, /item\.detail \? ' has-detail' : ''/)
  assert.match(css, /\.nc-item\.has-detail \.nc-item-main \{[^}]*flex-wrap: wrap/)
  assert.match(css, /\.nc-item-detail \{[^}]*flex-basis: 100%/)
  // Both model pickers carry the two fields, so the New chat menu and a
  // chat's chip read the same row the same way.
  for (const file of ['NewChat.jsx', 'ChatView.jsx']) {
    const view = source(file)
    assert.match(view, /detail: m\.detail \?\? null/, `${file} carries the detail`)
    assert.match(view, /note: m\.note \?\? null/, `${file} carries the note`)
  }
})

test('the picker footer offers a refresh with the capture age', () => {
  const newchat = source('NewChat.jsx')
  const css = source('newchat.css')
  // The age is the daemon's capture time, read through the one rule.
  assert.match(newchat, /import \{ relativeTime \} from '\.\/relative-time\.js'/)
  assert.match(newchat, /const catalogCapturedAt = currentHarness\?\.models_captured_at \?\? null/)
  assert.match(newchat, /Captured \$\{relativeTime\(/)
  assert.equal(
    `Captured ${relativeTime(
      Date.parse('2026-10-31T10:00:00Z'),
      Date.parse('2026-11-02T10:00:00Z'),
    )}`,
    'Captured 2 days ago',
  )
  // The foot: the age in its own line, then the button that captures again.
  // Only this console's own machine captures - another device's daemon runs
  // that harness's CLI itself.
  assert.match(newchat, /\{catalogCapturedAt && \(\s*<span className="nc-catalog-age"/)
  assert.match(
    newchat,
    /\{catalogCapturedAt && !target && \(\s*<button[\s\S]{0,160}onClick=\{refreshCatalog\}/,
  )
  assert.match(newchat, /'Refreshing…' : 'Refresh'/)
  assert.match(newchat, /const list = \(await refreshModels\(currentHarness\.id, target\)\) \?\? \[\]/)
  // A refusal is the daemon's own sentence in its own voice (a chat is
  // running, or the capture failed), through the one copy rule, never a
  // generic line about the request.
  assert.match(newchat, /setCatalogError\(errorCopy\(err\)\)/)
  assert.match(newchat, /\{catalogError \?\? `Captured/)
  // The age line sits above the footer buttons at caption size, and its
  // sentence can wrap when it is the daemon's own.
  assert.match(css, /\.nc-catalog-age \{[^}]*font-size: 11\.5px/)
})

test('a harness whose settings are live can change its model while a chat is up', () => {
  const chatview = source('ChatView.jsx')
  // The built-in map stays: those drivers take the words on a running chat.
  assert.match(chatview, /const LIVE_SETTINGS = \{/)
  // A plugin harness declares the fact on its own row. Without reading it,
  // the chip would stay a fact while the daemon was ready to take the change
  // on an idle chat, and any change at all during a turn.
  assert.match(chatview, /harness\?\.settings_live/)
  assert.match(chatview, /model: true, effort: false, fast: false/)
})

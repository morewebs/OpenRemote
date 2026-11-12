// Moving a chat between projects, as the views wire it: read from their
// source, since the console has no renderer under node:test. Each pin is a
// rule a regression would break for the person - the confirm, its words,
// the drop, the menus and the setting that turns the asking back on.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (name) => readFileSync(new URL(`./${name}`, import.meta.url), 'utf8')
const modal = read('MoveChatModal.jsx')
const hook = read('useMoveChat.jsx')
const sidebar = read('Sidebar.jsx')
const app = read('App.jsx')
const settings = read('SettingsView.jsx')

test('the confirm is a dialog like the others, and its failures are errorCopy', () => {
  assert.match(modal, /useDialog\(panelRef, onClose\)/)
  assert.match(modal, /useBack\(true, onClose\)/)
  assert.match(modal, /setError\(errorCopy\(err\)\)/)
  assert.doesNotMatch(modal, /err\.message/, 'never a raw error')
  // Move waits while a turn runs, and the checkbox is this device's answer.
  assert.match(modal, /disabled=\{busy \|\| Boolean\(blocked\)\}/)
  assert.match(modal, /if \(skip\) setAskBeforeMove\(false\)/)
  assert.match(modal, /Don&apos;t show this again/)
  // A project by id, never a folder.
  assert.match(modal, /moveChat\(chat\.id, to\?\.id \?\? null\)/)
})

test('every move asks first unless this device said not to', () => {
  assert.match(hook, /if \(askBeforeMove\(\)\) \{\s*setPending\(\{ move, failure: null \}\)/)
  assert.match(hook, /onNote\(errorCopy\(err\)\)/, 'an unasked failure says the daemon\'s words')
  // Another machine's chat moves among that machine's projects.
  assert.match(hook, /apiFor\(device\)\s*\.projects\(\)/)
})

test('the sidebar picker and a drop on a project both go through the confirm', () => {
  assert.match(sidebar, /const move = useMoveChat\(\{ onNote: setCopyNote \}\)/)
  assert.match(sidebar, /move\.moveTo\(row, project, place\)/, 'the picker')
  assert.match(sidebar, /if \(!dropMoves\(projects, row, project, row && move\.accessOf\(row\)\)\) return/, 'the drop')
  assert.match(sidebar, /\{\.\.\.dropTarget\(project, project\.id\)\}/, 'a project head takes a drop')
  assert.match(sidebar, /\{\.\.\.dropTarget\(null, 'flat'\)\}/, 'the Chats title is No project')
  assert.match(sidebar, /acceptsChatDrop\(e\.dataTransfer\?\.types\)/, 'only a dragged chat, never a text selection')
  assert.match(sidebar, /\{move\.modal\}/)
  assert.doesNotMatch(sidebar, /api\.call\('POST', `\/sessions\/\$\{row\.id\}\/workspace`/, 'one way to the route')
})

test('a pane\'s right-click menu moves its chat through a submenu', () => {
  assert.match(app, /chatMove\.menuItem\(\{ id: v, title, workspace: chat\.workspace \?\? null, executor: chat\.executor \?\? null \}\)/)
  assert.match(app, /chatMove\.prepare\(/)
  assert.match(app, /\{chatMove\.modal\}/)
  assert.match(hook, /return \{ label: 'Move to project', icon: <Folders size=\{14\} \/>, submenu \}/)
})

test('Settings > Confirmations turns the asking back on', () => {
  assert.match(settings, /data-stg="confirmations-move"/)
  assert.match(settings, /checked=\{prefs\.ask_move_chat\}/)
  assert.match(settings, /setPref\(\{ ask_move_chat: !prefs\.ask_move_chat \}\)/)
})

// The web console's shell, wired: the desktop views, Cloud only, no Tauri and
// no second renderer. Source text, like the repo's other wiring checks.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(file, import.meta.url), 'utf8')

test('the web entry mounts the desktop shell, not the old web-only view', () => {
  const main = read('../web-main.jsx')
  assert.match(main, /import WebShell from '\.\/web\/WebShell\.jsx'/)
  assert.match(main, /<WebShell/)
  assert.doesNotMatch(main, /DeviceView/)
})

test('the shell reuses the desktop sidebar, New chat and chat view', () => {
  const shell = read('WebShell.jsx')
  assert.match(shell, /import Sidebar from '\.\.\/Sidebar\.jsx'/)
  assert.match(shell, /import NewChat from '\.\.\/NewChat\.jsx'/)
  assert.match(shell, /import ChatView from '\.\.\/ChatView\.jsx'/)
  assert.match(shell, /mode="cloud"/)
  assert.match(shell, /machinesOnly/)
  assert.match(shell, /watchChats/)
  assert.doesNotMatch(shell, /from '\.\.\/TitleBar\.jsx'/)
  assert.doesNotMatch(shell, /@tauri-apps/)
})

test('the web provider shares the desktop console context', () => {
  const provider = read('console.jsx')
  assert.match(provider, /ConsoleContext/)
  assert.match(provider, /from '\.\.\/state\/console\.jsx'/)
  assert.match(provider, /WebConsoleProvider/)
})

test('the reused views know they are on the web', () => {
  const sidebar = read('../Sidebar.jsx')
  assert.match(sidebar, /web = false/)
  assert.match(sidebar, /!web/)
  const chat = read('../ChatView.jsx')
  assert.match(chat, /web = false/)
  const compose = read('../NewChat.jsx')
  assert.match(compose, /machinesOnly = false/)
  assert.match(compose, /defaultTarget = null/)
})

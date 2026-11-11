// Pictures in a chat and a composer's attachments, wired (br-images 2b770c1,
// 5f59a28 and the pictures half of 1a2ab3d). The views have no renderer in
// this suite, so the wiring is pinned by source text, like the repo's other
// wiring checks; the paths themselves are tested in composer-attachments,
// attachments, chat-pictures, the reducer and the API client.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test("picture support is read from the daemon's honors.attachments, never the branch's image_input", () => {
  for (const file of ['ChatView.jsx', 'NewChat.jsx', 'ComposerAttachments.jsx', 'composer-attachments.js']) {
    assert.doesNotMatch(read(file), /image_input/, `${file} reads image_input`)
  }
  assert.match(read('composer-attachments.js'), /row\.honors\?\.attachments === true/)
  assert.match(read('ChatView.jsx'), /takesPictures\(harness, chat\.harness\)/)
  assert.match(read('NewChat.jsx'), /takesPictures\(currentHarness, currentHarness\?\.id\)/)
})

test("a user's message and another agent's both show their pictures; the origin chip stays", () => {
  const view = read('ChatView.jsx')
  assert.match(view, /<ChatImages images=\{item\.images\} chat=\{chat\} label="Attached picture" \/>/)
  // Beside the OriginChip, in the same agent-message block.
  assert.match(
    view,
    /<OriginChip origin=\{item\.origin\}[^]*?<ChatImages images=\{item\.images\} chat=\{chat\} label="Picture from another agent" \/>/,
  )
  // A tool's screenshot rides its line in the work row.
  assert.match(view, /<ChatImages images=\{item\.images\} chat=\{chat\} label="Screenshot" \/>/)
  assert.match(view, /<WorkCluster key=\{item\.id\} items=\{item\.items\} running=\{chat\.running\} chat=\{chat\} \/>/)
})

test('both composers take paste, drop and the attach button, and show the chips', () => {
  for (const [file, key] of [
    ['ChatView.jsx', 'composerKey'],
    ['NewChat.jsx', "'new'"],
  ]) {
    const view = read(file)
    assert.match(view, /onPaste=\{attachInput\.onPaste\}/, `${file} paste`)
    assert.match(view, /onDrop=\{attachInput\.onDrop\}/, `${file} drop`)
    assert.match(view, /onDragOver=\{attachInput\.onDragOver\}/, `${file} drag over`)
    assert.match(view, /\{attachInput\.button\}/, `${file} attach button`)
    assert.match(view, /<AttachmentChips composerKey=/, `${file} chips`)
    assert.ok(view.includes(`useAttachInput(${key}`), `${file} input on its own composer key`)
  }
})

test('a no-picture app refuses a pasted or dropped picture in its own name', () => {
  assert.match(read('ChatView.jsx'), /refuse: takesImages \? null : takesNoPictures\(harnessText/)
  assert.match(read('NewChat.jsx'), /refuse: currentHarness && !takesImages \? takesNoPictures\(harnessLabel\(currentHarness\)\)/)
  const input = read('ComposerAttachments.jsx')
  assert.match(input, /onError\?\.\(errorCopy\(consoleError\(refuse\)\)\)/)
})

test("the browser's text bus and its picture bus follow the same focus", () => {
  const chat = read('ChatView.jsx')
  assert.match(chat, /onFocus=\{\(\) => \{\s*\n\s*claimText\(\)\s*\n\s*claimPictures\(\)/)
  assert.match(chat, /claims\.current\.pictures = setFocusedComposer\(composerKey\)/)
  const fresh = read('NewChat.jsx')
  assert.match(
    fresh,
    /releaseInserter\.current = claimInserter\(attach\)\s*\n\s*releasePictures\.current\?\.\(\)\s*\n\s*releasePictures\.current = setFocusedComposer\('new'\)/,
  )
})

test('both claims end when their composer goes, so nothing lands in a closed chat', () => {
  const chat = read('ChatView.jsx')
  assert.match(chat, /return \(\) => \{\s*\n\s*claims\.current\.text\?\.\(\)\s*\n\s*claims\.current\.pictures\?\.\(\)/)
  const fresh = read('NewChat.jsx')
  assert.match(fresh, /releasePictures\.current\?\.\(\)\s*\n\s*releasePictures\.current = null/)
})

test('sending refuses what the app cannot carry before anything runs, and says it through errorCopy', () => {
  const chat = read('ChatView.jsx')
  const prepare = chat.slice(chat.indexOf('const prepare = async'), chat.indexOf('const clearSent'))
  assert.ok(prepare.indexOf('outgoingOrThrow(') < prepare.indexOf('runCommand('), 'refused before any command runs')
  assert.match(chat, /queueMessage\(chat\.id, message, pointed, attachments\)/)
  assert.match(chat, /sendPrompt\(chat\.id, message, pointed, attachments\)/)
  assert.match(chat, /steerMessage\(chat\.id, message, pointed, attachments\)/)
  assert.equal((chat.match(/setError\(errorCopy\(err\)\)/g) ?? []).length, 2, 'send and steer')
  const fresh = read('NewChat.jsx')
  assert.match(fresh, /attachments: payload\.attachments/)
  assert.match(fresh, /moveAttachments\('new', id\)/)
})

test('a queued message shows its pictures, and Edit brings them back', () => {
  const chat = read('ChatView.jsx')
  assert.match(chat, /className="cv-queue-pics"/)
  assert.match(chat, /chatImageDataUrl\(apiFor\(elsewhere \? chat\.executor : null\), image, elsewhere \? chat\.id : null\)/)
})

test('the large view is one of the dialog family, and the webview can show and drop pictures', () => {
  const viewer = read('chat-images.jsx')
  assert.match(viewer, /useDialog\(panelRef, onClose\)/)
  assert.doesNotMatch(viewer, /'Escape'/)
  const conf = JSON.parse(read('../src-tauri/tauri.conf.json'))
  assert.match(conf.app.security.csp, /img-src 'self' data: blob:/)
  assert.equal(conf.app.windows[0].dragDropEnabled, false)
})

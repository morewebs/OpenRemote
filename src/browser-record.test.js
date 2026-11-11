// Recording a tab (ported from the browser branch's browser-record.js,
// 0d3211a): the shell's screencast pictures drawn into the system's
// MediaRecorder, one recording at a time, 90 seconds at most, saved to
// Downloads. The shell side (browser_screencast, browser_save_file,
// `browser:frame`) was already on main.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { recordingClock, recordingName } from './browser-chrome.js'
import { FIRST_FRAME_MS, LIMIT_MS, canRecord, recording, startRecording, stopRecording, subscribeRecording, tabClosed } from './browser-record.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

// A shell that answers every call and remembers it.
function fakeShell() {
  const calls = []
  return { calls, call: async (command, args) => (calls.push([command, args]), null) }
}

const ended = () => {
  let resolve
  const promise = new Promise((r) => (resolve = r))
  return { promise, done: (result) => resolve(result) }
}

test('a recording is named for when it started, with no colons, and its clock counts up', () => {
  assert.equal(recordingName(new Date(2026, 10, 11, 9, 5, 7)), 'OpenRemote recording 2026-11-11 09.05.07.webm')
  assert.equal(recordingClock(null, 5000), '0:00')
  assert.equal(recordingClock(1000, 66_000), '1:05')
  assert.equal(LIMIT_MS, 90_000)
  assert.equal(FIRST_FRAME_MS, 4000)
})

test('a window with no MediaRecorder records nothing and asks the shell for nothing', async () => {
  assert.equal(canRecord(), false)
  const shell = fakeShell()
  assert.equal(await startRecording('tab-1', shell.call, () => {}), false)
  assert.deepEqual(shell.calls, [])
  assert.equal(recording(), null)
})

test('a tab that sends no picture ends its recording in plain words, and the screencast stops', async () => {
  globalThis.MediaRecorder = class {}
  try {
    const shell = fakeShell()
    const end = ended()
    let heard = 0
    const stop = subscribeRecording(() => heard++)
    assert.equal(await startRecording('tab-2', shell.call, end.done, { firstFrameMs: 5 }), true)
    assert.deepEqual(recording(), { tab: 'tab-2', startedAt: null })
    // One at a time.
    assert.equal(await startRecording('tab-3', shell.call, () => {}), false)
    const result = await end.promise
    assert.deepEqual(result, { error: "This tab can't be recorded here yet." })
    assert.equal(recording(), null)
    assert.deepEqual(shell.calls, [
      ['browser_screencast', { tab: 'tab-2', on: true }],
      ['browser_screencast', { tab: 'tab-2', on: false }],
    ])
    assert.ok(heard >= 2)
    stop()
  } finally {
    delete globalThis.MediaRecorder
  }
})

test('the recorded tab closing ends it; another tab closing does not', async () => {
  globalThis.MediaRecorder = class {}
  try {
    const shell = fakeShell()
    const end = ended()
    await startRecording('tab-4', shell.call, end.done, { firstFrameMs: 60_000 })
    tabClosed('tab-9')
    assert.equal(recording()?.tab, 'tab-4')
    tabClosed('tab-4')
    assert.deepEqual(await end.promise, { error: 'Nothing was recorded: the page sent no picture yet.' })
    assert.equal(recording(), null)
    // Stopping with nothing running is nothing.
    await stopRecording()
  } finally {
    delete globalThis.MediaRecorder
  }
})

test('the store hears the pictures, ends a closed tab\'s recording, and lists the saved file', () => {
  const store = read('./browser-store.js')
  assert.match(store, /listen\('browser:frame', \(\{ payload \}\) => recordFrame\(payload\)\)/)
  const close = store.slice(store.indexOf('export function closePage'), store.indexOf('export function recordTab'))
  assert.match(close, /tabClosed\(native\)/)
  assert.match(store, /downloads: \[\{ id: `rec-\$\{Date\.now\(\)\}`, url: '', path: result\.path, state: 'done' \}/)
  const view = read('./BrowserView.jsx')
  assert.match(view, /useSyncExternalStore\(subscribeRecording, recording\)/)
  assert.match(view, /recordTab\(tab\.native, notify\)/)
  assert.match(view, /label: 'Record this tab'/)
  assert.match(view, /label: 'Stop recording'/)
  assert.match(view, /\{rec && <RecordPill rec=\{rec\} onStop=\{\(\) => stopRecording\(\)\} \/>\}/)
  // The shell commands are granted to the console's webview.
  const caps = read('../src-tauri/capabilities/default.json')
  assert.match(caps, /"allow-browser-screencast"/)
  assert.match(caps, /"allow-browser-save-file"/)
})

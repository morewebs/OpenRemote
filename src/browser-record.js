// Recording a tab: the page's screencast frames (`browser:frame`) drawn on
// a canvas that the system's own MediaRecorder turns into a WebM, so
// recording adds nothing to the app's size. One recording at a time,
// capped at 90 seconds, saved to Downloads. A plain module, so a recording
// keeps going while the user is elsewhere in the app. Ported from the
// browser branch (0d3211a); the shell's browser_screencast, browser_save_file
// and `browser:frame` were already on main.

import { recordingName } from './browser-chrome.js'

export const LIMIT_MS = 90_000
// A tab that sends no picture this long after recording starts can't be
// recorded (an engine without a screencast); the recording ends.
export const FIRST_FRAME_MS = 4000

let rec = null
let snapshot = null
const listeners = new Set()

function changed() {
  snapshot = rec ? { tab: rec.tab, startedAt: rec.startedAt } : null
  for (const fn of listeners) fn()
}

// The recording now, or null: { tab, startedAt } (startedAt is null until
// the first picture arrives).
export function recording() {
  return snapshot
}

export function subscribeRecording(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

function videoType() {
  for (const type of ['video/webm;codecs=vp9', 'video/webm;codecs=vp8', 'video/webm']) {
    if (globalThis.MediaRecorder?.isTypeSupported?.(type)) return type
  }
  return ''
}

// Whether this window can record at all (the system's MediaRecorder).
export function canRecord() {
  return typeof globalThis.MediaRecorder !== 'undefined'
}

// Start recording a tab. `call` reaches the shell; `done(result)` hears how
// it ended: { name, path } once saved, or { error } in plain words. False
// when nothing started: one is already running, or nothing can record here.
export async function startRecording(tab, call, done, { firstFrameMs = FIRST_FRAME_MS } = {}) {
  if (rec || !tab || !canRecord()) return false
  rec = { tab, call, done, startedAt: null, recorder: null, chunks: [], frame: null }
  rec.wait = setTimeout(() => {
    if (rec && !rec.recorder) finish({ error: "This tab can't be recorded here yet." })
  }, firstFrameMs)
  changed()
  await call('browser_screencast', { tab, on: true })
  return true
}

// The recorder starts on the first picture, at its size: a video's size
// can't change once it has begun.
function begin(width, height) {
  const canvas = document.createElement('canvas')
  // Encoders want even sizes.
  canvas.width = Math.max(2, Math.ceil(width / 2) * 2)
  canvas.height = Math.max(2, Math.ceil(height / 2) * 2)
  const stream = canvas.captureStream(30)
  const type = videoType()
  const recorder = new MediaRecorder(stream, { ...(type ? { mimeType: type } : {}), videoBitsPerSecond: 4_000_000 })
  recorder.ondataavailable = (e) => {
    if (e.data?.size) rec?.chunks.push(e.data)
  }
  recorder.start(1000)
  Object.assign(rec, { canvas, ctx: canvas.getContext('2d'), recorder, startedAt: Date.now() })
  // A page that doesn't change sends no pictures; drawing the last one
  // again keeps the video's clock running through the still moments.
  rec.tick = setInterval(() => {
    if (rec?.frame) rec.ctx.drawImage(rec.frame, 0, 0, rec.canvas.width, rec.canvas.height)
  }, 100)
  rec.limit = setTimeout(() => stopRecording(), LIMIT_MS)
  changed()
}

// One screencast picture from the shell.
export function recordFrame({ tab, data, width, height }) {
  if (!rec || tab !== rec.tab || !data) return
  const img = new Image()
  img.onload = () => {
    if (!rec) return
    if (!rec.recorder) begin(width || img.naturalWidth, height || img.naturalHeight)
    rec.frame = img
    rec.ctx.drawImage(img, 0, 0, rec.canvas.width, rec.canvas.height)
  }
  img.src = `data:image/jpeg;base64,${data}`
}

function base64Of(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ''))
    reader.onerror = () => reject(reader.error)
    reader.readAsDataURL(blob)
  })
}

async function finish(result) {
  const r = rec
  if (!r) return
  rec = null
  clearTimeout(r.wait)
  clearTimeout(r.limit)
  clearInterval(r.tick)
  changed()
  r.call('browser_screencast', { tab: r.tab, on: false })
  if (result) {
    r.done?.(result)
    return
  }
  // Stopped before the first picture: nothing was recorded, and it says so.
  if (!r.recorder) {
    r.done?.({ error: 'Nothing was recorded: the page sent no picture yet.' })
    return
  }
  const stopped = new Promise((resolve) => {
    r.recorder.onstop = resolve
  })
  r.recorder.stop()
  await stopped
  const blob = new Blob(r.chunks, { type: 'video/webm' })
  if (!blob.size) {
    r.done?.({ error: 'The recording came out empty.' })
    return
  }
  const name = recordingName(new Date(r.startedAt))
  const path = await r.call('browser_save_file', { name, data: await base64Of(blob) })
  r.done?.(path ? { name, path } : { error: "The recording couldn't be saved." })
}

// Stop and save (also what the 90-second cap does).
export function stopRecording() {
  return finish(null)
}

// A recorded tab closing ends its recording, saved as far as it went.
export function tabClosed(tab) {
  if (rec?.tab === tab) stopRecording()
}

// A composer's attachments (attachments.js keeps them) as a message
// carries them: pictures become the message's attachments; an element
// picked in the browser becomes words added to the message - where it came
// from and what it is - plus its picture. The limits match the daemon's
// (daemon/crates/openremote-daemon/src/images.rs).
//
// The ways in (paste, drop, the attach button) live here too, as plain
// functions over the event's files, so the paths are tested without a DOM;
// ComposerAttachments.jsx only hands them the events.

import { addAttachment, attachments } from './attachments.js'
import { consoleError, errorCopy } from './error-copy.js'

export const MAX_IMAGE_BYTES = 5 * 1024 * 1024
export const MAX_IMAGES = 10
// What the apps read; anything else is drawn again as one of these.
export const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp']
// A chat on another device is reached through /devices/{id} or the synced
// chat's forward, both capped at 4 MiB a request: this leaves room for the
// rest of the JSON body.
export const REMOTE_BYTES = 3_800_000
// The apps that read pictures, for a chat whose app this computer does not
// list (one running on another machine); the daemon's own
// `honors.attachments` decides wherever the row is known.
const PICTURE_APPS = new Set(['claude', 'codex'])
// A picture drawn smaller keeps at most this long side (what Claude reads
// in full; larger is scaled down on its side anyway).
const LONG_SIDE = 2576

/** Whether the chat's app reads pictures: the daemon's `honors.attachments`
 *  where its row is known, else the apps known to. */
export function takesPictures(row, appId) {
  if (row && typeof row === 'object') return row.honors?.attachments === true
  return PICTURE_APPS.has(appId)
}

/** A data URL's parts, `{ mime, data }` with the bytes in base64, or null. */
export function dataUrlParts(dataUrl) {
  if (typeof dataUrl !== 'string' || !dataUrl.startsWith('data:')) return null
  const comma = dataUrl.indexOf(',')
  if (comma === -1) return null
  const head = dataUrl.slice(5, comma)
  if (!head.endsWith(';base64')) return null
  return { mime: head.split(';')[0], data: dataUrl.slice(comma + 1) }
}

/** How many bytes a base64 string holds. */
export function base64Bytes(data) {
  const pad = data.endsWith('==') ? 2 : data.endsWith('=') ? 1 : 0
  return Math.floor((data.length * 3) / 4) - pad
}

/** The words a picked element adds to the message. */
export function elementText(item) {
  const where = item.title ? `${item.title} (${item.url})` : item.url
  return `Picked element from ${where}:\n${item.details ?? ''}`.trimEnd()
}

/** The sentence for an app that reads no pictures: at send, and when one is
 *  pasted or dropped on its composer. */
export const noPictures = (app) => `${app} doesn't read pictures - remove them to send.`
export const takesNoPictures = (app) => `${app} doesn't read pictures, so this chat can't take them`
export const TOO_MANY = `A message can carry ${MAX_IMAGES} pictures at most.`
export const TOO_BIG = 'A picture is over 5 MB - remove it or attach a smaller one.'
export const TOO_BIG_REMOTE =
  'Pictures for a chat on another device can add up to about 3.8 MB in one message - remove some, or send them one at a time.'

/**
 * What a composer sends: `{ text, attachments, error }`. The typed words
 * come first, then each picked element's; pictures go as attachments
 * (`{ kind: 'image', mime, data }`). For an app that reads no pictures
 * (`images: false`) an element goes as words alone, and pictures can't go
 * at all - `error` says so, and nothing is sent.
 */
export function outgoing(text, items, { images = true, app = 'This app', remote = false } = {}) {
  const blocks = []
  const sent = []
  let pictures = 0
  for (const item of items) {
    if (item.kind === 'element') {
      blocks.push(elementText(item))
      const parts = images ? dataUrlParts(item.dataUrl) : null
      if (parts) sent.push({ kind: 'image', mime: parts.mime, data: parts.data })
    } else if (item.kind === 'image') {
      pictures += 1
      const parts = dataUrlParts(item.dataUrl)
      if (parts) sent.push({ kind: 'image', mime: parts.mime, data: parts.data })
    }
  }
  const words = [String(text ?? '').trim(), ...blocks].filter(Boolean).join('\n\n')
  let error = null
  if (pictures && !images) error = noPictures(app)
  else if (sent.length > MAX_IMAGES) error = TOO_MANY
  else if (sent.some((a) => base64Bytes(a.data) > MAX_IMAGE_BYTES)) error = TOO_BIG
  else if (remote && sent.reduce((n, a) => n + a.data.length, words.length) > REMOTE_BYTES) error = TOO_BIG_REMOTE
  return { text: words, attachments: sent, error }
}

/** Throws the console's own error when the message can't carry what is attached. */
export function outgoingOrThrow(text, items, options) {
  const payload = outgoing(text, items, options)
  if (payload.error) throw consoleError(payload.error)
  return payload
}

// ---- the ways in ----

const isImage = (file) => typeof file?.type === 'string' && file.type.startsWith('image/')

/** The pictures a paste carries; empty when it carries none (text pastes go on as text). */
export function pastedPictures(event) {
  return [...(event?.clipboardData?.files ?? [])].filter(isImage)
}

/** Whether a paste carries words too (a copy from a document or a page
 *  often holds both): the text field pastes those itself. */
export function pastedWords(event) {
  try {
    return Boolean(event?.clipboardData?.getData?.('text/plain'))
  } catch {
    return false
  }
}

/**
 * What a composer does with a paste: `{ take, refuse, keepDefault }`. The
 * pictures are taken where the app reads them (`enabled`), else refused
 * with `refuse`'s sentence when there is one. Words riding with them are
 * never dropped: the field's own paste is held back (`keepDefault` false)
 * only when the paste is pictures alone and something was done with them.
 */
export function pastePlan(event, { enabled, refuse = null }) {
  const pictures = pastedPictures(event)
  const words = pastedWords(event)
  if (!pictures.length) return { take: [], refuse: false, keepDefault: true }
  if (enabled) return { take: pictures, refuse: false, keepDefault: words }
  if (!refuse) return { take: [], refuse: false, keepDefault: true }
  return { take: [], refuse: true, keepDefault: words }
}

/** The pictures a drop carries. */
export function droppedPictures(event) {
  return [...(event?.dataTransfer?.files ?? [])].filter(isImage)
}

/** Whether a drag holds files (a dragged chat or text does not). */
export function dragsFiles(event) {
  return [...(event?.dataTransfer?.types ?? [])].includes('Files')
}

/**
 * Adds the picked, pasted or dropped pictures to a composer, each made fit
 * to send by `convert` (imageAttachment). Stops at the message's limit;
 * each refusal reaches `onError` as errorCopy's sentence. Resolves to the
 * ids added.
 */
export async function takePictures(key, files, { convert = imageAttachment, onError = () => {} } = {}) {
  const added = []
  for (const file of files.filter(isImage)) {
    if (attachments(key).length >= MAX_IMAGES) {
      onError(errorCopy(consoleError(TOO_MANY)))
      break
    }
    try {
      added.push(addAttachment(await convert(file), key))
    } catch (err) {
      onError(errorCopy(err))
    }
  }
  return added
}

// ---- making a picture fit (browser only) ----

function readAsDataUrl(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result)
    reader.onerror = () => reject(reader.error)
    reader.readAsDataURL(blob)
  })
}

function encode(canvas, type, quality) {
  return new Promise((resolve) => canvas.toBlob(resolve, type, quality))
}

/**
 * A picture fit to send: as it is when the apps read its kind and it is
 * under the limit, else drawn again smaller - PNG kept as PNG where it
 * fits, JPEG otherwise.
 */
async function fitted(blob) {
  if (IMAGE_TYPES.includes(blob.type) && blob.size <= MAX_IMAGE_BYTES) return blob
  const bitmap = await createImageBitmap(blob)
  const scale = Math.min(1, LONG_SIDE / Math.max(bitmap.width, bitmap.height))
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(bitmap.width * scale))
  canvas.height = Math.max(1, Math.round(bitmap.height * scale))
  canvas.getContext('2d').drawImage(bitmap, 0, 0, canvas.width, canvas.height)
  bitmap.close?.()
  let out = blob.type === 'image/png' ? await encode(canvas, 'image/png') : null
  if (!out || out.size > MAX_IMAGE_BYTES) out = await encode(canvas, 'image/jpeg', 0.9)
  if (!out || out.size > MAX_IMAGE_BYTES) throw consoleError('This picture is over 5 MB even made smaller')
  return out
}

/** A pasted, dropped or picked file as a composer attachment. */
export async function imageAttachment(file) {
  let blob
  try {
    blob = await fitted(file)
  } catch (err) {
    if (err?.voice === 'console') throw err
    throw consoleError(`${file.name || 'That file'} isn't a picture that can be sent`)
  }
  return { kind: 'image', name: file.name || 'Pasted picture', mime: blob.type, dataUrl: await readAsDataUrl(blob) }
}

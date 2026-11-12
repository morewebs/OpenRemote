// The composer's attachments as a message carries them, and the ways in
// (paste, drop, the attach button's pick) - br-images 5f59a28, ported onto
// main's errorCopy and the daemon's honors.attachments.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { attachments, clearAttachments, removeAttachment } from './attachments.js'
import {
  MAX_IMAGES,
  TOO_BIG,
  TOO_BIG_REMOTE,
  base64Bytes,
  dataUrlParts,
  dragsFiles,
  droppedPictures,
  elementText,
  outgoing,
  outgoingOrThrow,
  pastedPictures,
  takePictures,
  takesNoPictures,
  takesPictures,
} from './composer-attachments.js'
import { consoleError, errorCopy } from './error-copy.js'

const png = (data = 'iVBORw0KGgo=') => ({
  kind: 'image',
  name: 'shot.png',
  mime: 'image/png',
  dataUrl: `data:image/png;base64,${data}`,
})
const file = (name, type = 'image/png') => ({ name, type })
const fit = async (f) => ({ kind: 'image', name: f.name, mime: f.type, dataUrl: `data:${f.type};base64,AAAA` })

test("picture support is the daemon's honors.attachments, with a fallback for an unlisted app", () => {
  assert.equal(takesPictures({ id: 'claude', honors: { attachments: true } }, 'claude'), true)
  assert.equal(takesPictures({ id: 'grok', honors: { attachments: false } }, 'grok'), false)
  assert.equal(takesPictures({ id: 'x', image_input: true }, 'x'), false, "the branch's image_input is not read")
  assert.equal(takesPictures(null, 'codex'), true)
  assert.equal(takesPictures(undefined, 'pi'), false)
})

test('a data URL splits into its type and base64 bytes, and the bytes are counted', () => {
  assert.deepEqual(dataUrlParts('data:image/png;base64,AAAA'), { mime: 'image/png', data: 'AAAA' })
  assert.equal(dataUrlParts('data:text/plain,hi'), null)
  assert.equal(dataUrlParts('https://x'), null)
  assert.equal(base64Bytes('AAAA'), 3)
  assert.equal(base64Bytes('AAA='), 2)
  assert.equal(base64Bytes('AA=='), 1)
})

test('pictures go as attachments after the words; a picked element goes as words plus its picture', () => {
  const element = {
    kind: 'element',
    label: 'Buy',
    url: 'https://shop.test/p',
    title: 'Shop',
    details: 'button "Buy"',
    dataUrl: 'data:image/jpeg;base64,BBBB',
  }
  const out = outgoing(' look at these ', [png(), element])
  assert.equal(out.error, null)
  assert.equal(out.text, `look at these\n\n${elementText(element)}`)
  assert.match(out.text, /Picked element from Shop \(https:\/\/shop\.test\/p\):\nbutton "Buy"/)
  assert.deepEqual(out.attachments, [
    { kind: 'image', mime: 'image/png', data: 'iVBORw0KGgo=' },
    { kind: 'image', mime: 'image/jpeg', data: 'BBBB' },
  ])
})

test('an app that reads no pictures is named, and its element goes as words alone', () => {
  const refused = outgoing('hi', [png()], { images: false, app: 'Grok Build' })
  assert.equal(refused.error, "Grok Build doesn't read pictures - remove them to send.")
  const element = { kind: 'element', label: 'Buy', url: 'https://shop.test', details: 'x', dataUrl: 'data:image/png;base64,AAAA' }
  const words = outgoing('hi', [element], { images: false, app: 'Grok Build' })
  assert.equal(words.error, null)
  assert.deepEqual(words.attachments, [])
})

test("the limits match the daemon: 10 pictures, 5 MB each, and Cloud's 4 MiB for another device", () => {
  const eleven = Array.from({ length: MAX_IMAGES + 1 }, () => png())
  assert.match(outgoing('', eleven).error, /10 pictures at most/)
  // 5 MB of bytes is about 6.7 MB of base64.
  const big = png('A'.repeat(Math.ceil(((5 * 1024 * 1024 + 4) * 4) / 3)))
  assert.equal(outgoing('', [big]).error, TOO_BIG)
  const twoMb = png('A'.repeat(2_000_000))
  assert.equal(outgoing('', [twoMb, twoMb]).error, null, 'fine for a chat here')
  assert.equal(outgoing('', [twoMb, twoMb], { remote: true }).error, TOO_BIG_REMOTE)
})

test("a refusal is the console's own error, which errorCopy repeats without quotes", () => {
  assert.throws(
    () => outgoingOrThrow('hi', [png()], { images: false, app: 'Pi Agent' }),
    (err) => {
      assert.equal(err.voice, 'console')
      assert.equal(errorCopy(err), "The console could not do this: Pi Agent doesn't read pictures - remove them to send.")
      return true
    },
  )
  assert.deepEqual(outgoingOrThrow('hi', []).attachments, [])
})

test('a paste or drop yields only its pictures; a dragged chat holds no files', () => {
  const paste = { clipboardData: { files: [file('a.png'), file('notes.txt', 'text/plain')] } }
  assert.deepEqual(pastedPictures(paste).map((f) => f.name), ['a.png'])
  assert.deepEqual(pastedPictures({ clipboardData: { files: [] } }), [])
  assert.deepEqual(pastedPictures({}), [])
  const drop = {
    dataTransfer: { files: [file('b.webp', 'image/webp'), file('c.pdf', 'application/pdf')], types: ['Files'] },
  }
  assert.deepEqual(droppedPictures(drop).map((f) => f.name), ['b.webp'])
  assert.equal(dragsFiles(drop), true)
  assert.equal(dragsFiles({ dataTransfer: { types: ['text/plain'] } }), false)
})

test('picked, pasted or dropped pictures land in that composer, and remove takes one out', async () => {
  clearAttachments('chat-a')
  const errors = []
  const ids = await takePictures(
    'chat-a',
    [file('one.png'), file('two.jpg', 'image/jpeg'), file('x.txt', 'text/plain')],
    { convert: fit, onError: (e) => errors.push(e) },
  )
  assert.equal(ids.length, 2)
  assert.deepEqual(errors, [])
  assert.deepEqual(attachments('chat-a').map((a) => a.name), ['one.png', 'two.jpg'])
  assert.deepEqual(attachments('chat-b'), [], 'another composer holds nothing')
  removeAttachment('chat-a', ids[0])
  assert.deepEqual(attachments('chat-a').map((a) => a.name), ['two.jpg'])
  clearAttachments('chat-a')
})

test('a composer stops at 10 pictures and says so through errorCopy', async () => {
  clearAttachments('chat-full')
  const errors = []
  const files = Array.from({ length: MAX_IMAGES + 2 }, (_, i) => file(`p${i}.png`))
  await takePictures('chat-full', files, { convert: fit, onError: (e) => errors.push(e) })
  assert.equal(attachments('chat-full').length, MAX_IMAGES)
  assert.deepEqual(errors, ['The console could not do this: A message can carry 10 pictures at most.'])
  clearAttachments('chat-full')
})

test("a picture that can't be made fit is refused early, in the console's words", async () => {
  clearAttachments('chat-big')
  const errors = []
  const tooBig = async () => {
    throw consoleError('This picture is over 5 MB even made smaller')
  }
  await takePictures('chat-big', [file('huge.png')], { convert: tooBig, onError: (e) => errors.push(e) })
  assert.deepEqual(attachments('chat-big'), [])
  assert.deepEqual(errors, ['The console could not do this: This picture is over 5 MB even made smaller.'])
})

test('a picture pasted or dropped on a no-picture app is refused in words naming the app', () => {
  assert.equal(
    errorCopy(consoleError(takesNoPictures('Grok Build'))),
    "The console could not do this: Grok Build doesn't read pictures, so this chat can't take them.",
  )
})

test('a paste of words and a picture keeps the words; the picture is taken or refused alone', async () => {
  const { pastePlan } = await import('./composer-attachments.js')
  const clip = (text, files) => ({ clipboardData: { files, getData: (t) => (t === 'text/plain' ? text : '') } })
  const both = clip('from the doc', [file('a.png')])
  // Where pictures are taken: the field pastes the words itself, the picture is attached.
  assert.deepEqual(pastePlan(both, { enabled: true, refuse: null }), { take: [file('a.png')], refuse: false, keepDefault: true })
  // A picture alone: nothing for the field to paste, so its default is held.
  const alone = clip('', [file('a.png')])
  assert.deepEqual(pastePlan(alone, { enabled: true, refuse: null }), { take: [file('a.png')], refuse: false, keepDefault: false })
  // Where pictures aren't taken: the words still paste, and the refusal is for the picture only.
  assert.deepEqual(pastePlan(both, { enabled: false, refuse: 'no' }), { take: [], refuse: true, keepDefault: true })
  assert.deepEqual(pastePlan(alone, { enabled: false, refuse: 'no' }), { take: [], refuse: true, keepDefault: false })
  // Words alone are the field's, untouched.
  assert.deepEqual(pastePlan(clip('hi', []), { enabled: true, refuse: null }), { take: [], refuse: false, keepDefault: true })
  assert.deepEqual(pastePlan(clip('hi', []), { enabled: false, refuse: 'no' }), { take: [], refuse: false, keepDefault: true })
  // Off with no sentence to give (a composer that isn't live): nothing is said, nothing held.
  assert.deepEqual(pastePlan(alone, { enabled: false, refuse: null }), { take: [], refuse: false, keepDefault: true })
})

test('the composer pastes through the plan, so words are never dropped with a picture', () => {
  const view = readFileSync(new URL('./ComposerAttachments.jsx', import.meta.url), 'utf8')
  assert.match(view, /const plan = pastePlan\(e, \{ enabled, refuse \}\)/)
  assert.match(view, /if \(!plan\.keepDefault\) e\.preventDefault\(\)/)
})

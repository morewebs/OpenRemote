// Screenshot and region to the composer (ported from the browser branch's
// 0d3211a: browser-attach.js, BrowserRegion and the view's shot/region): a
// picture of the page, or of the part dragged over, lands in the composer
// the user is in through attachments.js, and with no composer on screen the
// pane says so through errorCopy instead of keeping the picture anywhere.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { cameWhole, clipFor, regionScale, screenshotAttachment, tooSmall } from './browser-attach.js'
import { FLOATING, covered } from './browser-occlusion.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('a screenshot is a JPEG picture named for its site', () => {
  assert.deepEqual(screenshotAttachment('data:image/jpeg;base64,AA', 'https://www.example.com/a'), {
    kind: 'image',
    name: 'Screenshot of example.com.jpg',
    mime: 'image/jpeg',
    dataUrl: 'data:image/jpeg;base64,AA',
  })
  assert.equal(screenshotAttachment('data:x', '').name, 'Screenshot of the page.jpg')
})

test("the part dragged over is turned into the page's own pixels", () => {
  // Zoomed to 200% in a device view drawn at half size: one CSS pixel is one
  // screen pixel, so the clip is the drag.
  assert.equal(regionScale(2, 0.5), 1)
  assert.deepEqual(clipFor({ x: 10, y: 20, width: 100, height: 50 }, 2), { x: 5, y: 10, width: 50, height: 25 })
  assert.deepEqual(clipFor({ x: 10, y: 20, width: 100, height: 50 }, 0), { x: 10, y: 20, width: 100, height: 50 })
  assert.equal(regionScale(undefined, undefined), 1)
})

test('a whole-page answer to a clip is noticed, so the part is cut here', () => {
  const clip = { width: 100, height: 50 }
  const r = { width: 100, height: 50 }
  assert.equal(cameWhole({ width: 100, height: 50 }, clip, r, 1), false)
  assert.equal(cameWhole({ width: 200, height: 100 }, clip, r, 2), false, 'a sharper screen is still the part')
  assert.equal(cameWhole({ width: 1280, height: 720 }, clip, r, 1), true)
  assert.equal(cameWhole({ width: 100, height: 100 }, clip, r, 1), true, 'a different shape')
  assert.equal(cameWhole(null, clip, r, 1), true)
  assert.equal(tooSmall({ width: 3, height: 40 }), true)
  assert.equal(tooSmall({ width: 40, height: 40 }), false)
})

test('the region picker is a floating surface, so the page steps aside for it', () => {
  assert.ok(FLOATING.split(',').includes('.bw-region'))
  assert.equal(covered({ left: 0, top: 0, right: 100, bottom: 100, width: 100, height: 100 }, [{ left: 0, top: 0, right: 100, bottom: 100, width: 100, height: 100 }]), true)
})

test('the view sends pictures to the focused composer and says when there is none', () => {
  const view = read('./BrowserView.jsx')
  assert.match(view, /const \{ key \} = addToFocused\(item\)/)
  assert.match(view, /notify\(errorCopy\(err\)\)/)
  assert.match(view, /notify\(errorCopy\(consoleError\(NO_COMPOSER\)\)\)/)
  // Checked before anything is taken.
  const shot = view.slice(view.indexOf('const screenshot = async'), view.indexOf('const startRegion'))
  assert.ok(shot.indexOf('composerReady()') < shot.indexOf("call('browser_capture'"))
  assert.match(shot, /attachPicture\(screenshotAttachment\(dataUrl, tab\.url\), 'Screenshot'\)/)
  const region = view.slice(view.indexOf('const finishRegion'), view.indexOf('const toggleDevice'))
  assert.match(region, /call\('browser_capture', \{ tab: reg\.tab, clip \}\)/)
  assert.match(region, /if \(region && region\.tab !== current\?\.native\) setRegion\(null\)/)
  assert.match(view, /label: 'Screenshot to chat'/)
  assert.match(view, /label: 'Screenshot a region'/)
  assert.match(view, /<BrowserRegion box=\{region\.box\} onPick=\{finishRegion\} onCancel=\{\(\) => setRegion\(null\)\} \/>/)
  // A private tab's picture is never a visit: history stays the visit effect's.
  assert.doesNotMatch(shot + region, /recordVisit/)
})

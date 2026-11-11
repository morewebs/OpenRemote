// Full screen (ported from the browser branch's 0d3211a): a page showing
// something full screen (a video's button) gets the whole window. The shell
// already makes the window follow and says so (`browser:fullscreen`); the
// console hides its bars, keeps a site's request in view, and a page closed
// while full screen leaves it first.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { getState, onFullscreen } from './browser-store.js'

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('the store follows which page is full screen', () => {
  onFullscreen({ tab: 'tab-1', on: true })
  assert.equal(getState().fullscreen, 'tab-1')
  // Another page leaving says nothing about this one.
  onFullscreen({ tab: 'tab-2', on: false })
  assert.equal(getState().fullscreen, 'tab-1')
  onFullscreen({ tab: 'tab-1', on: false })
  assert.equal(getState().fullscreen, null)
})

test('the view gives the full-screen page the window and leaves full screen before closing it', () => {
  const store = read('./browser-store.js')
  assert.match(store, /listen\('browser:fullscreen', \(\{ payload \}\) => onFullscreen\(payload\)\)/)
  const view = read('./BrowserView.jsx')
  assert.match(view, /const full = Boolean\(fullscreen\) && fullscreen === current\?\.native/)
  assert.match(view, /className=\{full \? 'bw full' : 'bw'\}/)
  assert.match(view, /tab\.native === getState\(\)\.fullscreen\) \{\s+leaveFullscreen\(tab\.native\)\.finally\(\(\) => closeNow\(key\)\)/)
  assert.match(view, /expression: 'document\.exitFullscreen\?\.\(\)'/)
  const css = read('./browser.css')
  assert.match(css, /\.bw\.full \{\s+position: fixed;\s+inset: 0;/)
  // Every bar goes but a site's request.
  assert.match(css, /\.bw\.full > :not\(\.bw-page\):not\(\.bw-info\) \{\s+display: none;/)
  // The shell is what makes the window full screen.
  const shell = read('../src-tauri/src/browser/mod.rs')
  assert.match(shell, /window\.set_fullscreen\(on\)/)
})

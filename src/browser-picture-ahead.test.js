// The page's picture is taken ahead of what will cover it, so a console
// layer over the page shows the picture at once instead of waiting for one
// (pinned as source: the browser view has no renderer in tests).

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const view = readFileSync(new URL('./BrowserView.jsx', import.meta.url), 'utf8')
const bar = readFileSync(new URL('./BrowserBookmarksBar.jsx', import.meta.url), 'utf8')

test('focusing the address bar takes the picture ahead of the suggestions', () => {
  assert.match(view, /onFocus=\{\(e\) => \{\s*setEditing\(true\)\s*e\.target\.select\(\)[\s\S]{0,120}?precapture\(\)\s*\}\}/)
})

test('the star and Ctrl+D take the picture ahead of the bookmark editor', () => {
  // `star` is what the button and the Mod+D key both run.
  assert.match(view, /const star = \(\) => \{[\s\S]{0,200}?if \(editor \|\| !tab\?\.url\) return[\s\S]{0,120}?precapture\(\)/)
})

test('the tab strip, the toolbar and the bookmarks bar take it as the pointer arrives', () => {
  assert.match(view, /className="bw-tabs"[^>]*onPointerEnter=\{precapture\}/)
  assert.match(view, /className="bw-bar" onPointerEnter=\{precapture\}/)
  assert.match(bar, /aria-label="Bookmarks bar" onPointerEnter=\{precapture\}/)
})

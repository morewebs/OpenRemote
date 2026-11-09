// The device view as a product surface, wired: day groups, real-or-untitled
// titles, relative times, no per-row provenance or raw event count, and title
// hydration over the existing log route. Source text, like the repo's other
// wiring checks.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the list groups chats by day and never prints a raw id as the title', () => {
  const view = read('DeviceView.jsx')
  assert.match(view, /groupByDay\(sessions\)/)
  assert.match(view, /rowTitle\(row, titles\)/)
  assert.match(view, /dv-group-label/)
  assert.doesNotMatch(view, /Chat \$\{row\.id\.slice/)
  assert.doesNotMatch(view, /dv-row-head/)
})

test('the row line is the relative time, with the id only when untitled', () => {
  const view = read('DeviceView.jsx')
  assert.match(view, /relativeWhen/)
  assert.match(view, /rowLine\(row, \{/)
  assert.match(view, /shortId\(row\.id\)/)
  assert.doesNotMatch(view, /synced from this computer/)
})

test('titles are hydrated over the existing log route, capped', () => {
  const view = read('DeviceView.jsx')
  assert.match(view, /hydrateTitles\(/)
  assert.match(view, /\/sync\/sessions\/\$\{encodeURIComponent\(row\.id\)\}\/events\?from=0/)
  assert.match(view, /onTitle/)
})

test('the page has one honest line, not a banner, and holds the list while it loads', () => {
  const view = read('DeviceView.jsx')
  assert.doesNotMatch(view, /dv-honest/)
  assert.match(view, /dv-page-sub/)
  assert.match(view, /Read every few seconds, not a live stream/)
  assert.match(view, /dv-skeleton/)
})

test('the column and its rows are laid out as a list, not cards', () => {
  const css = read('device-view.css')
  assert.match(css, /max-width: 760px/)
  assert.match(css, /\.dv-group-label/)
  assert.match(css, /\.dv-skeleton/)
  assert.match(css, /\.dv-row-caret/)
  assert.match(css, /\.dv-composer/)
})

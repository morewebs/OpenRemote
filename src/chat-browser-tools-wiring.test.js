// The chat view's browser and team tool steps, pinned as source (the console
// has no renderer in tests). Main's quiet rail folds every tool into a work
// row, so the opened work row is where a step shows: its words, a browser
// or team tool's facts, and its Out through the one folding OutRow. The
// rule for what a step shows is workStep (quiet-rail.test.js).

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const view = readFileSync(new URL('./ChatView.jsx', import.meta.url), 'utf8')
const css = readFileSync(new URL('./chatview.css', import.meta.url), 'utf8')
const cluster = /function WorkCluster\([\s\S]*?\n\}\r?\n/.exec(view)?.[0] ?? ''

test('the opened work row shows each step through workStep', () => {
  assert.ok(cluster, 'WorkCluster is in the view')
  assert.match(view, /import \{ quietRail, workIsLive, workLabel, workStep \} from '\.\/quiet-rail\.js'/)
  assert.match(cluster, /const step = workStep\(item\)/)
  assert.match(cluster, /title=\{step\.browser \|\| team \? item\.name : undefined\}/)
  assert.match(cluster, /\{team \? team\.label : step\.label\}/)
})

test('a browser or team step shows its facts, and every step its Out through OutRow', () => {
  assert.match(cluster, /\{step\.browser && <BrowserFacts facts=\{step\.browser\} \/>\}/)
  assert.match(cluster, /teamTool\(item\.name, item\.input, \{ sessions, chatId: chat\.id, result: item\.result \}\)/)
  assert.match(cluster, /\{team && <TeamFacts facts=\{team\} \/>\}/)
  assert.match(cluster, /<OutRow raw=\{step\.out\} fold=\{step\.fold\} \/>/)
  // The open Out row reads through ToolIo (fields, or text behind its size).
  assert.match(view, /function OutRow\(\{ raw, fold \}\)[\s\S]*?<ToolIo label="Out" raw=\{raw\} \/>/)
})

test('the dead full tool card is gone; approvals keep the browser facts', () => {
  assert.doesNotMatch(view, /if \(item\.kind === 'tool'\) \{/)
  assert.doesNotMatch(view, /isThePendingAsk/)
  assert.match(view, /const browser = !question && isBrowserTool\(item\.toolName\) \? browserTool\(item\.toolName, item\.input\) : null/)
  assert.match(view, /\{keepBody && browser && <BrowserFacts facts=\{browser\} \/>\}/)
  assert.match(view, /!question && !command && !team && !browser && \(/)
  assert.match(view, /isBrowserTool\(item\.toolName\)\s*\? browserTool\(item\.toolName, item\.input\)\.label/)
})

test('the facts, the fold and the step body are styled', () => {
  for (const cls of ['.cv-bt {', '.cv-bt-url', '.cv-bt-detail', '.cv-fold {', '.cv-fold--under', '.cv-work-body {']) {
    assert.ok(css.includes(cls), `chatview.css styles ${cls}`)
  }
})

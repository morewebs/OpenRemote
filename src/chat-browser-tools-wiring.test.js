// The chat view's browser tool cards, pinned as source (the console has no
// renderer in tests): a browser call reads as "Browser · <verb>" with its
// facts in the work row, the tool card and the approval, and its Out row
// shares the one folding OutRow with the team tools.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const view = readFileSync(new URL('./ChatView.jsx', import.meta.url), 'utf8')
const css = readFileSync(new URL('./chatview.css', import.meta.url), 'utf8')

test('the chat view reads browser tools through browser-tools.js', () => {
  assert.match(view, /import \{ browserTool, foldsResult, isBrowserTool \} from '\.\/browser-tools\.js'/)
  assert.match(view, /function BrowserFacts\(\{ facts \}\)/)
})

test('the work row names a browser step by its label, the MCP name on hover', () => {
  assert.match(view, /title=\{isBrowserTool\(item\.name\) \? item\.name : undefined\}>\s*\{toolLabel\(item\)\}/)
})

test('an approval for a browser tool shows its facts, not the JSON input', () => {
  assert.match(view, /const browser = !question && isBrowserTool\(item\.toolName\) \? browserTool\(item\.toolName, item\.input\) : null/)
  assert.match(view, /\{keepBody && browser && <BrowserFacts facts=\{browser\} \/>\}/)
  assert.match(view, /!question && !command && !team && !browser && \(/)
  assert.match(view, /isBrowserTool\(item\.toolName\)\s*\? browserTool\(item\.toolName, item\.input\)\.label/)
})

test('the tool card: label, facts, no In dump, and one folding Out row for every tool', () => {
  assert.match(view, /const shownIn = item\.input && !command && !edit && !team && !browser \? item\.input : null/)
  assert.match(view, /\{team \? team\.label : browser \? browser\.label : item\.name\}/)
  assert.match(view, /\{browser && <BrowserFacts facts=\{browser\} \/>\}/)
  assert.match(view, /<OutRow raw=\{shownOut\} fold=\{Boolean\(browser\) && foldsResult\(item\.name\)\} \/>/)
  assert.doesNotMatch(view, /\{shownOut && <ToolIo label="Out"/)
  // The open Out row still reads through ToolIo (fields, or text behind its size).
  assert.match(view, /function OutRow\(\{ raw, fold \}\)[\s\S]*?<ToolIo label="Out" raw=\{raw\} \/>/)
})

test('the facts and the fold are styled', () => {
  for (const cls of ['.cv-bt {', '.cv-bt-url', '.cv-bt-detail', '.cv-fold {', '.cv-fold--under']) {
    assert.ok(css.includes(cls), `chatview.css styles ${cls}`)
  }
})

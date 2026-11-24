import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const read = (name) => readFileSync(new URL(name, import.meta.url), 'utf8')

test('both desktop panes rename their own chat; WebShell has no rename dialog', () => {
  const app = read('./App.jsx')
  assert.match(app, /<ChatView key=\{chat\.id\}[^\n]*\n\s*onRename=\{\(\) => openRename\(chat\.id\)\}/)
  assert.match(app, /<ChatView key=\{paneChat\.id\}[^\n]*\n\s*onRename=\{\(\) => openRename\(paneChat\.id\)\}/)
  assert.doesNotMatch(read('./web/WebShell.jsx'), /onRename=/)
  assert.match(read('./ChatView.jsx'), /<ChatHeader[\s\S]*onRename=\{onRename\}/)
})

test('the run controls use distinct calls and do not share the sending flag', () => {
  const view = read('./ChatView.jsx')
  assert.match(view, /const runControl = \(action\) => control\(action, \{ report: setRunError, lockComposer: false \}\)/)
  assert.match(view, /const interruptReply = \(\) => runControl\(\(\) => interruptChat\(chat\.id\)\)/)
  assert.match(view, /const endRun = \(\) => runControl\(\(\) => stopChat\(chat\.id\)\)/)
  assert.match(view, /<ChatSendActions[\s\S]*onResume=\{\(\) => runControl\(\(\) => resumeChat\(chat\.id\)\)\}/)
  const actions = read('./ChatSendActions.jsx')
  assert.match(actions, /className="nc-send cv-run-button" onClick=\{onInterrupt\}/)
  assert.match(actions, /onClick=\{onSend\} disabled=\{!canSend\}/)
  assert.match(actions, /<ContextMenu anchor=\{anchor\} anchorSide="above" label="Run actions"/)
  assert.doesNotMatch(read('./ChatHeader.jsx'), /onInterrupt|onResume|End run/)
})

test('header actions reuse the existing menu, copy and details flows', () => {
  const view = read('./ChatView.jsx')
  assert.match(read('./ChatHeader.jsx'), /<ContextMenu anchor=\{anchor\}/)
  assert.match(view, /onCopy=\{\(\) => headerControl\(copyConversation\)\}/)
  assert.match(view, /if \(name === 'export'\) return copyConversation\(\)/)
  assert.match(view, /onDetails=\{\(\) => setSlashDialog\('status'\)\}/)
  assert.match(view, /<StatusModal chat=\{chat\} app=\{harnessText\}\s+device=\{where\} modelLabel=/)
  const statusModal = read('./SlashDialogs.jsx').split('export function StatusModal')[1]
  assert.match(statusModal, /useEscLayer\(true, onClose\)/)
  assert.match(statusModal, /return createPortal\([\s\S]*document\.body,/)
})

test('the send control transition respects both reduced-motion settings', () => {
  const css = read('./chatview.css')
  assert.match(css, /animation: cv-send-appear/)
  assert.match(css, /@media \(prefers-reduced-motion: reduce\) \{\s*\.cv-send-actions > \.nc-send,\s*\.cv-stop-group > \.cv-run-button \{\s*animation: none/)
  assert.match(css, /html\[data-reduce-motion\] \.cv-send-actions > \.nc-send,\s*html\[data-reduce-motion\] \.cv-stop-group > \.cv-run-button \{\s*animation: none/)
})

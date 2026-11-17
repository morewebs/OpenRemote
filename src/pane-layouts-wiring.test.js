// Workspace layouts have one entry point in the sidebar and share the
// existing picker engine. The pure layout rules live in layouts.test.js.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')
const area = source('SplitArea.jsx')
const css = source('split.css')
const panel = source('WorkspaceLayouts.jsx')
const sidebar = source('Sidebar.jsx')

test('the sidebar owns one workspace layout control and pane headers keep only pane actions', () => {
  assert.equal((sidebar.match(/aria-label="Workspace layout"/g) ?? []).length, 1)
  assert.match(sidebar, /aria-haspopup="menu" aria-expanded=\{workspaceLayoutOpen\}/)
  assert.match(sidebar, /onClick=\{onWorkspaceLayout\}/)
  const actions = area.slice(area.indexOf('<div className="split-actions">'), area.indexOf('</header>'))
  assert.match(actions, /Maximize pane/)
  assert.match(actions, /Close this pane/)
  assert.doesNotMatch(actions, /Layouts|Split|split-btn--text/)
  assert.match(panel, /<PickerMenu[\s\S]*?presentation="actions"[\s\S]*?hideSearch/)
  assert.match(panel, /createPortal\(/)
})

test('the panel reads the saved list and uses the pure layout helpers', () => {
  assert.match(panel, /import \{[^}]*loadLayouts[^}]*offerLayouts[^}]*\} from '\.\/layouts\.js'/)
  assert.match(panel, /const offers = offerLayouts\(loadLayouts\(\), \{ currentChat \}\)/)
  assert.match(panel, /const candidate = presetLayout\(layout, preset\)/)
  assert.match(panel, /const fits = layoutFits\(candidate, width, height\)/)
  assert.doesNotMatch(panel, /restoreShape\(/, 'the shell owns restoring a saved layout')
})

test('each layout carries its pane count, and one that cannot open says why', () => {
  assert.match(panel, /detail: offer\.countLine/)
  assert.match(panel, /const fits = offer\.ok && layoutFits\(offer\.plan\.layout, width, height\)/)
  assert.match(panel, /plan: offer\.plan, disabled: !fits/)
  assert.match(panel, /note: offer\.note \?\?/)
  assert.match(panel, /if \(!item \|\| item\.disabled\) return/)
  assert.match(panel, /onRestore\(item\.plan\)/)
  assert.match(panel, /if \(!offers\.length\) items\.push\(\{[^\n]*name: SAVED_LAYOUTS_NONE/)
})

test('the empty state is one sentence, shared with the palette\u2019s layouts page', () => {
  const layouts = source('layouts.js')
  assert.match(layouts, /export const SAVED_LAYOUTS_NONE = 'No saved layouts yet'/)
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /SAVED_LAYOUTS_NONE, capacityLine, loadLayouts/)
  // The layouts page takes the specific sentence; the generic one stays for
  // pages that really have nothing at all.
  assert.match(palette, /page === 'layouts'\s*\n\s*\? SAVED_LAYOUTS_NONE/)
  assert.match(palette, /'Nothing to show here yet\.'/)
  // And no second copy of the sentence anywhere the two of them say it.
  const copies = (panel.match(/'No saved layouts yet'/g) ?? []).length
  assert.equal(copies, 0, 'the workspace panel reads the constant, not a copy')
})

test('save-as is offered only when the window holds more than one pane', () => {
  assert.match(panel, /if \(count > 1\) items\.push\([\s\S]*?name: 'Save layout…'/)
  assert.match(panel, /id === 'save'\) onSave\(\)/)
})

test('the shell callbacks reach the centralized panel rather than every pane', () => {
  assert.match(sidebar, /!isMobile && onWorkspaceLayout && <button/)
  assert.match(area, /onSaveLayout = null,/)
  assert.match(area, /onRestoreLayout = null,/)
  assert.match(area, /currentChat = null,/)
  assert.match(area, /\{layoutControl && <WorkspaceLayouts/)
  assert.match(area, /onSave=\{onSaveLayout\} onRestore=\{onRestoreLayout\}/)
  assert.match(source('App.jsx'), /onWorkspaceLayout=\{openWorkspaceLayout\}/)
})

test('the panel has compact preset tiles while pane actions retain desktop targets', () => {
  const panelCss = source('workspace-layouts.css')
  assert.match(panelCss, /width: 304px/)
  assert.match(panelCss, /grid-template-columns: repeat\(4, minmax\(0, 1fr\)\)/)
  assert.match(css, /\.split-btn \{[^}]*width: 32px; height: 32px/)
})

// ---- what the header says ----

test('a chat pane header shows the chat, a role pane keeps the role name', () => {
  // The record comes from the console, the same source the sidebar reads.
  assert.match(area, /import \{ useConsole \} from '\.\/state\/console\.jsx'/)
  assert.match(area, /import \{ RECONNECTING, paneChatId, paneHeader \} from '\.\/pane-header\.js'/)
  assert.match(area, /const \{ chats, projects, reconnecting \} = useConsole\(\)/)
  assert.match(area, /const chatId = paneChatId\(node\.view, currentChat\)/)
  assert.match(area, /const head = paneHeader\(\{/)
  assert.match(area, /projectFor\(projects \?\? \[\], chat\.workspace\)/)
})

test('the header wears the sidebar dot and the sidebar harness mark, one line', () => {
  // Same classes and statuses as the sidebar's row: one language, not two.
  assert.match(area, /className=\{`dot dot--\$\{head\.status\}`\}/)
  assert.match(area, /<HarnessMark harness=\{head\.harness\} size=\{16\} \/>/)
  // The title truncates while the controls keep their width.
  assert.match(area, /<span className="split-title">\{head\.title\}<\/span>/)
  assert.match(area, /<span className="split-project">\{head\.project\}<\/span>/)
  assert.match(css, /\.split-title \{[\s\S]{0,140}text-overflow: ellipsis/)
  assert.match(css, /\.split-project \{[\s\S]{0,200}text-overflow: ellipsis/)
  assert.match(css, /\.split-actions \{\s*flex: none/, 'the actions never give ground to the title')
  // A role pane renders none of the chat parts: no dot, no mark, no project.
  assert.match(area, /\{head\.chat && <span className=\{`dot dot--/)
  assert.match(area, /\{head\.chat && head\.harness && <HarnessMark/)
  assert.match(area, /\{head\.project && <span className="split-project">/)
})

test('a pane whose stream is between attempts says reconnecting beside the title', () => {
  // The console mirrors the stream's retry per chat; the header reads it, and
  // the word sits beside the title. The dot is untouched: it still renders the
  // chat's status, and the transport is not a status.
  const area = source('SplitArea.jsx')
  assert.match(area, /import \{ RECONNECTING, paneChatId, paneHeader \} from '\.\/pane-header\.js'/)
  assert.match(area, /const \{ chats, projects, reconnecting \} = useConsole\(\)/)
  assert.match(area, /reconnecting: chatId \? Boolean\(reconnecting\?\.\[chatId\]\) : false,/)
  assert.match(area, /\{head\.reconnecting && <span className="split-reconnecting">\{RECONNECTING\}<\/span>\}/)
  assert.match(area, /className=\{`dot dot--\$\{head\.status\}`\}/, 'the dot still speaks only for the status')
  const css = source('split.css')
  assert.match(css, /\.split-reconnecting \{[\s\S]{0,160}white-space: nowrap;/)
})

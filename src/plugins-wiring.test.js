// The plugins, MCP and endpoint panes are browser surfaces: their guards
// are one-line conditions inside components that this suite cannot render
// (there is no test renderer in this repo). So the wiring is pinned the
// way the other wiring tests here pin it - the expression that makes the
// behaviour possible has to be present - and the pure part of each guard
// is unit tested in row-confirm.test.js.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the read path refuses a second press while one is already running', () => {
  const modal = source('InstallPluginModal.jsx')
  // The button is disabled while busy, but Enter on the source field reaches
  // read() without passing it. The guard is in read() itself, so the key path
  // and the button obey the same rule; the comment between the two guards says
  // why, so the window the pattern allows covers it.
  assert.match(modal, /const read = async \(source\) => \{\s*\n\s*if \(!source\) return[\s\S]{0,400}?if \(busy\) return/)
})

test('a dialog that is installing refuses to close, and says why', () => {
  const modal = source('InstallPluginModal.jsx')
  // Esc, the backdrop and the X all call close(); the guard is inside it.
  // The dialog family's shared plumbing owns the Escape layer now, and it is
  // handed the same close, so the guard still stands in front of the key.
  assert.match(modal, /const close = \(\) => \{\s*\n\s*if \(busy\) return/)
  assert.match(modal, /useDialog\(panelRef, close\)/)
  assert.match(modal, /if \(e\.target === e\.currentTarget\) close\(\)/)
  // A guard that silently swallows the X is dead UI: while busy the control
  // is disabled and its title and accessible name say the step has to finish.
  assert.match(modal, /className="dv-modal-close"\s*\n\s*onClick=\{close\}\s*\n\s*disabled=\{busy\}/)
  assert.match(modal, /title=\{closeTitle\}/)
  assert.match(modal, /aria-label=\{closeTitle\}/)
  assert.match(modal, /const closeTitle = busy\s*\n\s*\? 'Wait for this step to finish or fail/)
  // And the note under the head explains the three inert paths in words.
  assert.match(modal, /\{busy && \(\s*\n\s*<p className="pg-note" role="status">/)
  // The backdrop handler is a deliberate no-op while busy, and the comment
  // says so: it is not a handler nobody wired.
  assert.match(modal, /Deliberate no-op while busy, not a dead handler/)
})

test('the Discover tab stays mounted, so its search and filters survive a trip to Installed', () => {
  const view = source('PluginsView.jsx')
  assert.doesNotMatch(view, /tab === 'discover' &&/)
  assert.match(view, /hidden=\{tab !== 'discover'\}/)
  assert.match(view, /<PluginDiscover onInstall=/)
})

test('a failed plugin refresh is kept and shown with a retry', () => {
  const view = source('PluginsView.jsx')
  const console = source('state/console.jsx')
  assert.match(view, /marketplacesError/)
  // One row, naming whichever surface failed; its retry refreshes that one and
  // both when both are stale.
  assert.match(view, /onClick=\{retryFailed\}/)
  assert.match(view, /if \(pluginsError\) await refreshPlugins\(\)/)
  assert.match(view, /if \(marketplacesError\) await refreshMarketplaces\(\)/)
  // Each surface clears only its own failure: a plugin refresh that succeeds
  // says nothing about the marketplaces, so its clear names its own field.
  assert.equal((console.match(/setPluginsError\(null\)/g) ?? []).length, 1)
  assert.equal((console.match(/setMarketplacesError\(null\)/g) ?? []).length, 1)
  assert.equal((console.match(/setPluginsError\(errorCopy\(err\)\)/g) ?? []).length, 1)
  assert.equal((console.match(/setMarketplacesError\(errorCopy\(err\)\)/g) ?? []).length, 1)
  assert.match(console, /pluginsError,\s*\n\s*marketplacesError,\s*\n\s*refreshPlugins,/)
})

test('the installed Remove button is disabled while its own removal runs', () => {
  const view = source('PluginsView.jsx')
  assert.match(view, /disabled=\{removing === p\.id\}/)
  assert.match(view, /setRemoving\(p\.id\)/)
  assert.match(view, /removing === p\.id \? 'Removing…' : 'Remove it'/)
  // No second removal can start behind the first: every other row's
  // first press is disabled while one is in flight.
  assert.match(view, /disabled=\{removing != null\}/)
})

test('an endpoint removal is a two-step confirm, and every other press disarms it', () => {
  const pane = source('EndpointManager.jsx')
  assert.match(pane, /import \{ armRow, clearRow, rowIsArmed \} from '\.\/row-confirm\.js'/)
  assert.match(pane, /setArmed\(\(curr\) => armRow\(curr, entry\.id\)\)/)
  assert.match(pane, /rowIsArmed\(armed, entry\.id\)/)
  // The daemon call exists once, in the armed branch, so the first press
  // cannot reach it. The window is wide enough for the arm watch's own two
  // attributes, which sit between the class and the press on purpose.
  assert.equal((pane.match(/remove\(entry\.id\)/g) ?? []).length, 1)
  assert.match(pane, /className="pn-btn pn-btn--danger"[\s\S]{0,220}remove\(entry\.id\)/)
  // Editing, testing, enabling and defaulting all disarm the row.
  assert.equal((pane.match(/setArmed\(clearRow\(\)\)/g) ?? []).length, 7)
  // The arm also ends when the row stops being the thing the user is looking
  // at: the armed button watches its own visibility and its focus, and the
  // shared hook ends the arm on either (use-armed-row.js, pinned below). An
  // arm that survives a scroll is a first press nobody remembers making.
  assert.match(pane, /import \{ useArmedRow \} from '\.\/use-armed-row\.js'/)
  assert.match(pane, /const armWatch = useArmedRow\(armed, setArmed\)/)
  assert.match(pane, /ref=\{armWatch\.buttonRef\}/)
  assert.match(pane, /onBlur=\{armWatch\.onBlur\}/)
})

test('an MCP server removal is a two-step confirm too', () => {
  const pane = source('McpServersSection.jsx')
  assert.match(pane, /import \{ armRow, clearRow, rowIsArmed \} from '\.\/row-confirm\.js'/)
  assert.match(pane, /setArmed\(\(curr\) => armRow\(curr, p\.id\)\)/)
  assert.match(pane, /rowIsArmed\(armed, p\.id\)/)
  assert.equal((pane.match(/remove\(p\.id\)/g) ?? []).length, 1)
  assert.match(pane, /className="pn-btn pn-btn--danger"[\s\S]{0,220}remove\(p\.id\)/)
  // Enable, disable, add-key and removal each disarm first.
  assert.equal((pane.match(/setArmed\(clearRow\(\)\)/g) ?? []).length, 2)
  // Same arm lifetime as the endpoint pane: out of view or out of focus ends
  // it, so a scroll cannot leave the second press loaded.
  assert.match(pane, /import \{ useArmedRow \} from '\.\/use-armed-row\.js'/)
  assert.match(pane, /const armWatch = useArmedRow\(armed, setArmed\)/)
  assert.match(pane, /ref=\{armWatch\.buttonRef\}/)
  assert.match(pane, /onBlur=\{armWatch\.onBlur\}/)
})

test('an armed row ends when it leaves the viewport or loses focus', () => {
  const hook = source('use-armed-row.js')
  // Scrolled out of view: an IntersectionObserver on the armed button itself,
  // armed by a callback ref so it follows the row rather than the list. The id
  // is read at event time (`armedRef.current`) because an observer callback
  // that lands after a re-render must compare against the arm that is live
  // now, not the one that was under it when the observer was built.
  assert.match(hook, /new IntersectionObserver\(/)
  assert.match(hook, /!entry\.isIntersecting/)
  assert.match(hook, /armAfter\(current, armedRef\.current, \{ visible: false \}\)/)
  // Focus leaving ends it too, and the id is read at event time here as well:
  // a stale blur must not disarm the row armed a moment later.
  assert.match(hook, /armAfter\(current, armedRef\.current, \{ focused: false \}\)/)
  // The armed control takes the keyboard when it replaces the trash, so the
  // keyboard path is not dropped back to the start of the document.
  assert.match(hook, /el\.focus\?\.\(\{ preventScroll: true \}\)/)
  // The observer is taken down on unmount and when another row arms, so no
  // stale observer can disarm a row the user is now looking at.
  assert.match(hook, /\.disconnect\?\.\(\)/)
  // Both events ask the tested rule in row-confirm.js rather than inventing a
  // second copy of the condition.
  assert.doesNotMatch(hook, /setArmed\(clearRow\(\)\)/)
  // Every armed destructive row in the console watches its arm.
  for (const pane of ['EndpointManager.jsx', 'McpServersSection.jsx', 'PluginsView.jsx', 'AutomationsView.jsx']) {
    assert.match(source(pane), /useArmedRow\(/, `${pane} must watch its armed row`)
  }
})

test('a plugin tile is drawn from local marks, and fetches nothing', () => {
  const icon = source('PluginIcon.jsx')
  // The tile resolved a plugin's homepage into a github.com avatar or a Google
  // favicon request, so simply listing plugins told those hosts which ones this
  // computer had. The desktop shell's CSP refused the request; the shipped web
  // console has no CSP, so it went out. Identity is local now, and the three
  // fallbacks the catalog relies on stay in order: brand mark, category,
  // monogram.
  assert.doesNotMatch(icon, /https?:\/\//)
  assert.doesNotMatch(icon, /<img/)
  assert.doesNotMatch(icon, /const remote =/)
  assert.match(icon, /if \(brand && BRAND_ICONS\[brand\]\)/)
  assert.match(icon, /const CategoryIcon = catKey \? CATEGORY_ICONS\[catKey\] : null/)
  assert.match(icon, /className="pg-icon-monogram"/)
  // No caller hands the tile a homepage any more: the prop is gone from its
  // signature, so a caller that still passed one would be dead code waiting to
  // be believed.
  const signature = icon.match(/export function PluginIcon\(\{[\s\S]*?\}\)/)
  assert.ok(signature, 'PluginIcon has a props signature')
  assert.doesNotMatch(signature[0], /homepage/)
  for (const caller of ['PluginCard.jsx', 'PluginDetailModal.jsx', 'PluginsView.jsx']) {
    const element = source(caller).match(/<PluginIcon[\s\S]*?\/>/)
    assert.ok(element, `${caller} draws a plugin tile`)
    assert.doesNotMatch(element[0], /homepage/, `${caller} still passes a homepage`)
  }
})

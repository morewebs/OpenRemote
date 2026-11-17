// The shell's wiring: every component App.jsx renders with a contract of its
// own must be handed the props that contract needs.
//
// Why a source-text check is the honest level for this. The components own
// their rules and test them; the shell owns the call sites and nothing tests
// them; and nothing else in the tree reads both halves at once. So a component
// can be built, tested, and never wired: the pane header's Layouts menu was
// passed to the wrong component for hours - SplitArea got the layout props
// while the pane that draws the header did not - and every suite stayed green,
// because the pane's own tests exercised the props it was given and the shell
// had no test at all. Reading App.jsx against each component's own declaration
// is the only check that can see that class of break, and it costs nothing to
// run.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

// A comment cannot pass a prop. Strip both kinds before anything is read, so
// a sentence inside a destructuring block or a JSX tag is never mistaken for
// an argument (`// The shell hosts the rename dialog...` used to appear as a
// "required prop" when this check was a scratch script).
function stripComments(text) {
  let out = ''
  let i = 0
  let quote = null
  while (i < text.length) {
    const c = text[i]
    if (quote) {
      out += c
      if (c === '\\') {
        out += text[i + 1] ?? ''
        i += 2
        continue
      }
      if (c === quote) quote = null
      i += 1
      continue
    }
    if (c === '"' || c === "'" || c === '`') {
      quote = c
      out += c
      i += 1
      continue
    }
    if (c === '/' && text[i + 1] === '/') {
      while (i < text.length && text[i] !== '\n') i += 1
      continue
    }
    if (c === '/' && text[i + 1] === '*') {
      i += 2
      while (i < text.length && !(text[i] === '*' && text[i + 1] === '/')) i += 1
      i += 2
      continue
    }
    out += c
    i += 1
  }
  return out
}

/** What the shell imports as a component: name -> module path it came from. */
function importedComponents(text) {
  const map = new Map()
  for (const m of text.matchAll(/^import\s+([A-Za-z0-9_]+)\s+from\s+'(\.\/[^']+)'/gm)) {
    if (/^[A-Z]/.test(m[1])) map.set(m[1], m[2])
  }
  for (const m of text.matchAll(/^import\s+\{([^}]+)\}\s+from\s+'(\.\/[^']+)'/gm)) {
    for (const part of m[1].split(',')) {
      const name = part.trim().split(/\s+as\s+/).pop()
      if (name && /^[A-Z]/.test(name)) map.set(name, m[2])
    }
  }
  return map
}

/** Every `<Name ...>` the source writes, with the props each one passes. */
function callSites(text, name) {
  const clean = stripComments(text)
  const sites = []
  let from = 0
  for (;;) {
    const at = clean.indexOf(`<${name}`, from)
    if (at < 0) break
    const boundary = clean[at + 1 + name.length]
    if (boundary !== undefined && !/[\s/>]/.test(boundary)) {
      from = at + 1
      continue
    }
    let i = at + 1 + name.length
    let depth = 0
    let quote = null
    for (; i < clean.length; i += 1) {
      const c = clean[i]
      if (quote) {
        if (c === '\\') i += 1
        else if (c === quote) quote = null
        continue
      }
      if (c === '"' || c === "'" || c === '`') {
        quote = c
        continue
      }
      if (c === '{') depth += 1
      else if (c === '}') depth -= 1
      else if (c === '>' && depth === 0) break
    }
    const tag = clean.slice(at, i + 1)
    const props = new Set()
    for (const m of tag.matchAll(/(?:^|\s)([a-zA-Z][a-zA-Z0-9]*)\s*=/g)) props.add(m[1])
    const spread = /\{\s*\.\.\.[A-Za-z0-9_]+\s*\}/.test(tag)
    const selfClosing = /\/\s*>$/.test(tag)
    const body = !selfClosing && !/^\s*$/.test(tag.slice(tag.indexOf('>') + 1).replace(/\/\s*>$/, ''))
    sites.push({ line: clean.slice(0, at).split('\n').length, props, spread, body })
    from = i + 1
  }
  return sites
}

/** A component's own declaration: which props it needs, which it can skip. */
function contractOf(file, name) {
  const text = stripComments(source(file))
  const at = text.search(new RegExp(`export default function ${name}\\s*\\(`))
  if (at < 0) return null
  const open = text.indexOf('(', at)
  let depth = 0
  let close = open
  for (let i = open; i < text.length; i += 1) {
    const c = text[i]
    if (c === '(') depth += 1
    else if (c === ')') {
      depth -= 1
      if (depth === 0) {
        close = i
        break
      }
    }
  }
  const args = text.slice(open + 1, close).trim()
  if (!args.startsWith('{')) return []
  let braces = 0
  let end = 0
  for (let i = 0; i < args.length; i += 1) {
    if (args[i] === '{') braces += 1
    else if (args[i] === '}') {
      braces -= 1
      if (braces === 0) {
        end = i
        break
      }
    }
  }
  const inner = args.slice(1, end)
  const entries = []
  let nest = 0
  let current = ''
  for (const ch of inner) {
    if ('{[('.includes(ch)) nest += 1
    if ('}])'.includes(ch)) nest -= 1
    if (ch === ',' && nest === 0) {
      entries.push(current)
      current = ''
    } else {
      current += ch
    }
  }
  entries.push(current)
  return entries
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const rest = entry.startsWith('...')
      const name = entry.split('=')[0].split(':')[0].replace('...', '').trim()
      const optional = rest || entry.includes('=') || name === 'children'
      return { name, optional }
    })
}

// The contract is read from each component's own destructuring, so there is no
// list here to keep in step: a prop added to a component's signature is checked
// at every call site the next time this file runs, and a prop given a default
// stops being required. `children` is never an attribute - a tag with a body
// satisfies it - and a rest element is satisfied by the component, not by the
// shell.
const REQUIRED = (file, name) => (contractOf(file, name) ?? []).filter((p) => !p.optional).map((p) => p.name)

// ---- the three contracts the shell has the most trouble keeping ----

test('App hands SplitArea its pane and centralized layout contracts', () => {
  const app = source('App.jsx')
  const sites = callSites(app, 'SplitArea')
  assert.ok(sites.length > 0, 'the shell renders SplitArea')
  // Defaulted callbacks can disappear silently, so check the layout panel's
  // opening, closing and actions as well as the pane renderer's arguments.
  const needed = ['layout', 'setLayout', 'renderPane', 'labelFor', 'onSaveLayout', 'onRestoreLayout', 'currentChat', 'onClosePane', 'layoutControl', 'onCloseLayoutControl']
  for (const site of sites) {
    for (const prop of needed) {
      assert.ok(
        site.props.has(prop),
        `App.jsx:${site.line} renders SplitArea without ${prop}`,
      )
    }
  }
  // Pane controls stay local; workspace layout actions reach the one panel.
  const inner = callSites(source('SplitArea.jsx'), 'Pane')
  assert.ok(inner.length > 0, 'SplitArea renders a Pane')
  for (const site of inner) {
    for (const prop of ['currentChat', 'onStartDrag', 'onZoom', 'onClose', 'onSplit', 'onDropInPane']) {
      assert.ok(
        site.props.has(prop) || site.spread,
        `SplitArea.jsx:${site.line} renders Pane without ${prop}`,
      )
    }
  }
  const panels = callSites(source('SplitArea.jsx'), 'WorkspaceLayouts')
  assert.equal(panels.length, 1, 'SplitArea renders one centralized layout panel')
  for (const prop of REQUIRED('WorkspaceLayouts.jsx', 'WorkspaceLayouts')) {
    assert.ok(panels[0].props.has(prop), `SplitArea renders WorkspaceLayouts without ${prop}`)
  }
})

test('App hands CommandPalette its rename and layout props', () => {
  const sites = callSites(source('App.jsx'), 'CommandPalette')
  assert.ok(sites.length > 0, 'the shell renders CommandPalette')
  const needed = ['onClose', 'onOpenChat', 'onSaveLayout', 'onRestoreLayout', 'onRenameChat', 'focusedChatId', 'panes', 'paneCapacity', 'raceFree']
  for (const site of sites) {
    for (const prop of needed) {
      assert.ok(site.props.has(prop), `App.jsx:${site.line} renders CommandPalette without ${prop}`)
    }
  }
})

test('every ContextMenu the shell draws gets its items and its onClose', () => {
  const sites = callSites(source('App.jsx'), 'ContextMenu')
  assert.ok(sites.length > 0, 'the shell draws a context menu')
  for (const site of sites) {
    for (const prop of ['x', 'y', 'items', 'onClose']) {
      assert.ok(site.props.has(prop), `App.jsx:${site.line} draws a ContextMenu without ${prop}`)
    }
  }
})

// ---- the rest of the shell, from each component's own declaration ----

// Components whose declaration says "required" while the prop is really
// optional, and which the shell therefore does not pass. Each entry names its
// owner: the repair is in the component or in the shell, never here - this list
// exists so the sweep below is green on today's tree and still fails on a new
// miss. A test at the end of this file checks the list has not gone stale.
const DECLARED_NOT_PASSED = new Map([
  [
    'ChatView.onBack',
    'ChatView declares onBack in its props and never reads it (the shell never passes one); owner: ChatView',
  ],
])

test('every component the shell renders with required props gets them all', () => {
  const app = source('App.jsx')
  const components = importedComponents(app)
  const missing = []
  for (const [name, file] of components) {
    const sites = callSites(app, name)
    if (!sites.length) continue
    const required = REQUIRED(file, name)
    if (!required.length) continue
    for (const site of sites) {
      if (site.spread) {
        missing.push(`${name}@${site.line}: passes its props as a spread, so this check cannot see them`)
        continue
      }
      for (const prop of required) {
        if (prop === 'children' && site.body) continue
        if (!site.props.has(prop)) missing.push(`${name}.${prop}@${site.line}`)
      }
    }
  }
  const unexpected = missing.filter((key) => !DECLARED_NOT_PASSED.has(key.split('@')[0]))
  assert.deepEqual(
    unexpected,
    [],
    `the shell renders a component without a prop its declaration requires:\n  ${unexpected.join('\n  ')}`,
  )
})

test('the known list still names something real', () => {
  const app = source('App.jsx')
  const components = importedComponents(app)
  const found = new Set()
  for (const [name, file] of components) {
    const sites = callSites(app, name)
    if (!sites.length) continue
    for (const prop of REQUIRED(file, name)) {
      for (const site of sites) {
        if (!site.spread && !site.props.has(prop) && !(prop === 'children' && site.body)) {
          found.add(`${name}.${prop}`)
        }
      }
    }
  }
  for (const key of DECLARED_NOT_PASSED.keys()) {
    assert.ok(
      found.has(key),
      `${key} is on the known list but nothing misses it any more - delete its entry at the top of src/shell-contract-wiring.test.js`,
    )
  }
})

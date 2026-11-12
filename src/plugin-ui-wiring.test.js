// The JavaScript plugin runtime's wiring: the parts that live inside
// components and the shell's policy, which this suite cannot render. Each pin
// is the expression a security promise rests on - the frame's sandbox, the
// console's CSP, the message gate, the token staying out of the frame, a
// project's plugin staying in its project, and a click before any write.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync, readdirSync } from 'node:fs'
import { buttonWords } from './plugin-runtime.js'
import { SECTIONS } from './settings-search.js'

const read = (name) => readFileSync(new URL(`./${name}`, import.meta.url), 'utf8')

test('every plugin frame is made in one place, sandboxed to scripts alone', () => {
  const host = read('plugins-host.jsx')
  const frames = host.match(/<iframe[\s\S]*?\/>/g) ?? []
  assert.equal(frames.length, 1, 'one iframe element, the one every plugin page runs in')
  assert.match(frames[0], /sandbox=\{FRAME_SANDBOX\}/)
  assert.match(frames[0], /src=\{host\.frameSrc\}/)
  assert.match(frames[0], /allow=\{allow\}/)
  assert.match(host, /allow=\{frameAllow\(record\.plugin\.powers\)\}/)
  // Nothing in the console widens a sandbox to the same origin.
  for (const file of readdirSync(new URL('./', import.meta.url))) {
    if (!/\.(jsx?|html)$/.test(file) || file.endsWith('.test.js')) continue
    const text = read(file).replace(/^\s*\/\/.*$/gm, '')
    assert.doesNotMatch(text, /allow-same-origin/, file)
  }
})

test("the console's CSP names only the app's own origins", () => {
  const conf = JSON.parse(read('../src-tauri/tauri.conf.json'))
  const csp = conf.app.security.csp
  const policy = Object.fromEntries(
    csp
      .split(';')
      .map((d) => d.trim().split(/\s+/))
      .map(([name, ...sources]) => [name, sources]),
  )
  // The app's own: itself, the shell's origins on each system, what it makes
  // (blob:, data:) and the shell's IPC. Nothing else may appear.
  const own = new Set([
    "'self'",
    'blob:',
    'data:',
    'ipc:',
    'http://ipc.localhost',
    'http://tauri.localhost',
    'https://tauri.localhost',
    'tauri://localhost',
  ])
  for (const [name, sources] of Object.entries(policy)) {
    for (const source of sources) {
      if (own.has(source)) continue
      // Inline styles, which the console's components set.
      if (name === 'style-src' && source === "'unsafe-inline'") continue
      // This computer's own daemon, whose port is chosen at start.
      if (name === 'connect-src' && source === 'http://127.0.0.1:*') continue
      assert.fail(`${name} names ${source}, which is not the app's own`)
    }
  }
  // Plugin frames are blob: pages, their scripts blob: scripts made inside
  // them; the bridge is the app's own file under each shell origin.
  assert.deepEqual(policy['script-src'], ["'self'", 'http://tauri.localhost', 'https://tauri.localhost', 'tauri://localhost', 'blob:'])
  assert.deepEqual(policy['frame-src'], ["'self'", 'blob:'])
  assert.deepEqual(policy['media-src'], ["'self'", 'blob:'])
  assert.ok(!policy['script-src'].includes("'unsafe-eval'") && !policy['script-src'].includes("'unsafe-inline'"))
  assert.doesNotMatch(csp, /(^|\s)\*(\s|;|$)|https:(\s|;|$)|http:(\s|;|$)/, 'no wildcard source, no bare scheme')
})

test('the bridge never holds the token or reaches the daemon itself', () => {
  // The code, not its comments (which say what it never holds).
  const bridge = read('../public/plugin-bridge.js').replace(/^\s*\/\/.*$/gm, '')
  assert.doesNotMatch(bridge, /\btoken\b|Authorization|Bearer|localStorage|sessionStorage|127\.0\.0\.1|document\.cookie/i)
  // It reaches nothing by itself: no fetch, XHR or socket of its own.
  assert.doesNotMatch(bridge, /(^|[^.\w])fetch\(|XMLHttpRequest|WebSocket|EventSource|sendBeacon/)
  // It answers only its parent.
  assert.match(bridge, /if \(event\.source !== host\) return/)
  const host = read('plugins-host.jsx')
  const calls = read('plugin-calls.js')
  for (const [name, text] of [
    ['plugins-host.jsx', host],
    ['plugin-calls.js', calls],
  ]) {
    assert.doesNotMatch(text, /\.token\b|Authorization|localStorage/, name)
    // What a plugin may do is its install record's, never its folder's.
    assert.doesNotMatch(text, /openremote\.json/, name)
  }
  // The frame is posted to by its own window only, and handed its replies.
  assert.match(host, /frame\.win\.postMessage\(/)
  assert.doesNotMatch(host, /window\.parent|\.postMessage\([^)]*location/)
})

test('a message is answered only after the gate names its frame', () => {
  const host = read('plugins-host.jsx')
  assert.match(
    host,
    /onMessageRef\.current = async \(event\) => \{\s*\n\s*const frame = frameOf\(event, framesRef\.current\)\s*\n\s*if \(!frame\) return/,
  )
  // A call is answered only through the record's own check.
  assert.match(host, /const reply = await answerCall\(record, message, callsRef\.current\)/)
  // A key from a frame reaches the app only through the key rule, with the
  // frame's own element as the focus test.
  assert.match(host, /if \(!keyPassesUp\(action, \{ view: frame\.place\.kind === 'view', focused: focusedHere \}\)\) return/)
  assert.match(host, /document\.activeElement === frame\.el/)
  // The frames are keyed by the very window the iframe made.
  assert.match(host, /const win = iframe\.contentWindow\s*\n\s*if \(!win\) return undefined/)
  assert.match(host, /framesRef\.current\.set\(win, frame\)/)
})

test("a project's plugin shows only in its project's composers and sidebar", () => {
  const buttons = read('PluginComposerButtons.jsx')
  assert.match(buttons, /const project = projectFor\(projects, workspace\)\?\.id \?\? null/)
  assert.match(buttons, /const shown = host\.inputsFor\(project\)/)
  const sidebar = read('Sidebar.jsx')
  assert.match(sidebar, /const pluginViews = pluginHost\?\.viewsFor\(contextProject\) \?\? \[\]/)
  const host = read('plugins-host.jsx')
  assert.match(host, /views\.filter\(\(v\) => ridesProject\(v\.plugin, projectId\)\)/)
  assert.match(host, /inputs\.filter\(\(i\) => ridesProject\(i\.plugin, projectId\)\)/)
  // Each composer says whose chat it is.
  assert.match(read('ChatView.jsx'), /<PluginComposerButtons[\s\S]{0,200}workspace=\{chat\.workspace \?\? null\}/)
  assert.match(read('NewChat.jsx'), /<PluginComposerButtons[\s\S]{0,200}workspace=\{workspace\}/)
})

test('a write needs a click: the grant is made in the click handler and nowhere else', () => {
  const host = read('plugins-host.jsx')
  assert.equal((host.match(/grantClick\(/g) ?? []).length, 1)
  assert.match(host, /const clickInput = useCallback\(\(pluginId, inputId, handle\) => \{\s*\n\s*grantClick\(grantsRef\.current, pluginId, handle\)/)
  const buttons = read('PluginComposerButtons.jsx')
  assert.equal((buttons.match(/clickInput\(/g) ?? []).length, 1)
  assert.match(buttons, /onClick=\{\(\) => host\.clickInput\(pluginId, input\.id, handle\)\}/)
  // No other file grants anything.
  for (const file of readdirSync(new URL('./', import.meta.url))) {
    if (!/\.jsx?$/.test(file) || file.endsWith('.test.js') || file === 'plugin-calls.js' || file === 'plugins-host.jsx') continue
    assert.doesNotMatch(read(file), /grantClick/, file)
  }
  // And a plugin never sends: the composer's own send is not among its calls.
  assert.doesNotMatch(read('plugin-calls.js'), /sendPrompt|createChat|onSend|submit/)
})

test("a plugin's button carries its plugin's name", () => {
  const words = buttonWords({ label: 'Send' }, 'Board', null)
  assert.equal(words.label, 'Send (Board plugin)')
  assert.equal(words.title, 'Send (Board plugin)')
  const busy = buttonWords({ label: 'Dictate' }, 'Dictation', { label: '0:04', title: 'Listening' })
  assert.equal(busy.label, 'Dictate (Dictation plugin): 0:04')
  assert.equal(busy.title, 'Listening (Dictation plugin)')
  const buttons = read('PluginComposerButtons.jsx')
  assert.match(buttons, /aria-label=\{words\.label\}/)
  assert.match(buttons, /title=\{words\.title\}/)
  assert.match(buttons, /className="plg-input"/)
})

test('Settings > Plugins is a registered tab with its own panel', () => {
  const section = SECTIONS.find((s) => s.id === 'plugins')
  assert.ok(section, 'the tab is in settings-search.js')
  assert.equal(section.label, 'Plugins')
  const view = read('SettingsView.jsx')
  assert.match(view, /plugins: PuzzlePiece/)
  assert.match(view, /id="stg-plugins"[\s\S]{0,200}hidden=\{!!query \|\| active !== 'plugins'\}/)
  assert.match(view, /<PluginSettings \/>/)
  assert.match(view, /plugins: pluginSettingsList/)
  // A failed load or save reads in the console's own voice.
  const form = read('PluginSettings.jsx')
  assert.match(form, /setLoadError\(errorCopy\(err\)\)/)
  assert.match(form, /setError\(errorCopy\(err\)\)/)
  assert.doesNotMatch(form, /err\.message/)
})

test("a plugin's view is a pane like a chat, known by its key", () => {
  const app = read('App.jsx')
  assert.match(app, /const isChatView = \(v\) => !STATIC_VIEWS\.includes\(v\) && !isPluginView\(v\)/)
  assert.match(app, /isPluginView\(view\) \? \(\s*\n\s*\/\/[^\n]*\n\s*<PluginView viewKey=\{view\} \/>/)
  assert.match(app, /if \(isPluginView\(leaf\.view\)\) return <PluginView viewKey=\{leaf\.view\} \/>/)
  assert.match(app, /<PluginProvider>/)
})

// The JavaScript plugin runtime's rules, kept free of React and the DOM so
// they are tested directly: what an installed plugin's record contributes to
// the console, which calls a plugin's powers answer, which messages count as
// a frame's own, where text a plugin writes lands in a composer, and the page
// every plugin frame boots from.
//
// What a plugin may do is read from its install record - the powers and the
// contributions the user approved on the install screen - never from the
// plugin's files as they are now. The daemon checks the same record again on
// every route a frame's call reaches.

import { projectFor } from './projects.js'

// The one sandbox a plugin frame gets: scripts, and nothing else. Never
// allow-same-origin (the frame would share the console's origin, its storage
// and the daemon token in it), never forms, pop-ups or top navigation.
export const FRAME_SANDBOX = 'allow-scripts'

// The icons a plugin may name for its sidebar tile and composer button,
// drawn from the console's own set (plugin-icons.jsx). A manifest names one
// as a word (`microphone`, `Microphone` or `magic-wand`); any other gets the
// plugin piece.
export const PLUGIN_ICONS = [
  'microphone',
  'waveform',
  'translate',
  'sparkle',
  'magicwand',
  'notepencil',
  'notebook',
  'bookopen',
  'listchecks',
  'chatcircle',
  'camera',
  'image',
  'paperclip',
  'globe',
  'code',
  'terminal',
  'robot',
  'lightning',
  'clock',
  'calendar',
  'chartbar',
  'puzzlepiece',
]

export function iconName(value) {
  const key = typeof value === 'string' ? value.toLowerCase().replace(/[^a-z]/g, '') : ''
  return PLUGIN_ICONS.includes(key) ? key : 'puzzlepiece'
}

// The calls a frame's API makes that reach the chats, each carried to the
// daemon's own method (the same gate a process plugin's program meets), and
// the power each one needs. The list is closed: a call not here or below is
// refused before anything is asked.
export const CHAT_CALLS = {
  'chats.list': 'chats/list',
  'chats.read': 'chats/read',
  'chats.start': 'chats/start',
  'chats.steer': 'chats/prompt',
  'chats.stop': 'chats/stop',
}

export const CALL_POWER = {
  'chats.list': 'read_chats',
  'chats.read': 'read_chats',
  'chats.start': 'agent_tools',
  'chats.steer': 'agent_tools',
  'chats.stop': 'agent_tools',
  fetch: 'network',
}
// Calls that need nothing beyond being the plugin's own: its settings (the
// secrets only as set or unset), its files, its notes and its own views.
export const OWN_CALLS = new Set(['settings.get', 'files.read', 'ui.toast', 'ui.openView', 'ui.openSettings'])
// Calls into a composer, for a plugin that adds composer buttons - and then
// only into a composer whose button the user clicked (the host's grant).
export const COMPOSER_CALLS = new Set(['composer.insert', 'composer.attach', 'composer.setState'])

export const ALL_CALLS = new Set([...Object.keys(CALL_POWER), ...OWN_CALLS, ...COMPOSER_CALLS])

const POWER_WORDS = {
  run_programs: 'run programs',
  read_chats: 'read chats',
  agent_tools: 'start, steer and stop chats',
  network: 'reach the internet',
  microphone: 'use the microphone',
}

// The kinds of message a frame's bridge sends.
const FRAME_KINDS = new Set(['ready', 'booted', 'log', 'failed', 'key', 'call'])

// A path inside the plugin's folder, the way the daemon's files route takes
// it: relative, forward slashes, never climbing out.
export function insidePath(path) {
  return (
    typeof path === 'string' &&
    path.length > 0 &&
    path.length <= 512 &&
    !path.startsWith('/') &&
    !path.includes('\\') &&
    !path.includes(':') &&
    path.split('/').every((part) => part !== '..' && part !== '.' && part !== '')
  )
}

const text = (value, max = 60) => (typeof value === 'string' && value.trim() ? value.trim().slice(0, max) : null)

// A plugin's name as people read it: its name with the first letter raised
// and dashes as spaces ("voice-notes" reads "Voice notes").
export function displayName(plugin) {
  const name = plugin?.name ?? ''
  return name ? name[0].toUpperCase() + name.slice(1).replace(/-/g, ' ') : 'Plugin'
}

// The settings a plugin declares, as the form draws them, from the install
// record (the daemon checked them at review).
export function readSettings(list) {
  const out = []
  for (const raw of Array.isArray(list) ? list : []) {
    if (!raw || typeof raw !== 'object' || typeof raw.key !== 'string') continue
    out.push({
      key: raw.key,
      type: raw.type,
      label: text(raw.label, 60) ?? raw.key,
      help: text(raw.help, 300),
      placeholder: text(raw.placeholder, 80),
      default: raw.default ?? null,
      options: (Array.isArray(raw.options) ? raw.options : [])
        .filter((o) => o && typeof o.value === 'string')
        .map((o) => ({ value: o.value, label: text(o.label, 60) ?? o.value })),
      min: typeof raw.min === 'number' ? raw.min : null,
      max: typeof raw.max === 'number' ? raw.max : null,
      required: Boolean(raw.required),
    })
  }
  return out
}

// What an installed plugin adds to the console, read from its install
// record: the page its composer buttons run (its entry), the buttons, the
// views it opens in a pane (those placed in the sidebar), and its settings.
// A button whose page is missing is left out - a control that can do
// nothing is never drawn.
export function contributionsOf(plugin) {
  const contributes = plugin?.contributes && typeof plugin.contributes === 'object' ? plugin.contributes : {}
  const entry = insidePath(plugin?.entry) ? plugin.entry : null
  const inputs = []
  for (const item of Array.isArray(contributes.composer_inputs) ? contributes.composer_inputs : []) {
    if (!item || typeof item.id !== 'string') continue
    const page = insidePath(item.entry) ? item.entry : entry
    if (!page) continue
    inputs.push({ id: item.id, label: text(item.label, 40) ?? item.id, icon: iconName(item.icon), entry: page })
  }
  const views = []
  for (const item of Array.isArray(contributes.views) ? contributes.views : []) {
    if (!item || typeof item.id !== 'string' || item.place !== 'sidebar' || !insidePath(item.entry)) continue
    views.push({ id: item.id, title: text(item.title, 48) ?? item.id, icon: iconName(item.icon), entry: item.entry })
  }
  return { entry, inputs, views, settings: readSettings(contributes.settings) }
}

// Whether a plugin is for chats of `projectId` (null: no project): a Global
// or Machine plugin is for every chat, a project's plugin only for that
// project's - the daemon's own rule (scope::rides).
export function ridesProject(plugin, projectId) {
  const scope = plugin?.scope ?? 'machine'
  if (scope !== 'project' && scope !== 'project_local') return true
  return Boolean(plugin.project) && plugin.project === projectId
}

// Why a frame's call is refused, in plain words, or null when the plugin may
// make it. `plugin` is { name, powers, inputs } - the powers from the
// install record.
export function refusal(call, { name = 'This plugin', powers = [], inputs = [] } = {}) {
  if (typeof call !== 'string' || !ALL_CALLS.has(call)) return `There is no openremote call named ${call}.`
  const power = CALL_POWER[call]
  if (power) {
    return powers.includes(power)
      ? null
      : `${name} may not ${POWER_WORDS[power]}: that power was not approved when it was installed.`
  }
  if (COMPOSER_CALLS.has(call)) {
    return inputs.length ? null : `${name} adds nothing to the composer, so it can't write in one.`
  }
  return null
}

// The frame a message came from, or null when it is none of ours. A message
// counts only from a frame the host made - the very window, which is the
// frame's contentWindow - with the opaque origin every sandboxed frame has
// ("null"), and only in the bridge's own shape. Anything else (another
// window, the console itself, a frame that somehow got an origin) is
// ignored.
export function frameOf(event, frames) {
  if (!event || !frames || typeof frames.get !== 'function') return null
  const frame = event.source ? frames.get(event.source) : null
  if (!frame || event.origin !== 'null') return null
  const message = event.data
  if (!message || typeof message !== 'object' || message.openremote !== 1) return null
  if (!FRAME_KINDS.has(message.kind)) return null
  if (message.kind === 'call' && (!Number.isSafeInteger(message.id) || typeof message.method !== 'string')) return null
  return frame
}

// The app's keys a focused plugin view may pass up, by name: ones that only
// move around the app. The list is explicit so that it fails closed - an
// action added to the app later passes only once it is added here. A key
// that acts on a chat (steer, stop, archive, a picker) or closes something
// is never passed, so a plugin's page can't press it for the user; nor is
// any key from a hidden page, or from a view that does not have the focus
// (the bridge's message is the plugin's own word, not proof of a key press).
export const PLUGIN_KEY_ACTIONS = new Set([
  'new-chat',
  'palette',
  'cheat-sheet',
  'search-chats',
  'next-chat',
  'prev-chat',
  'first-chat',
  'last-chat',
  'view-settings',
  'view-automations',
  'view-plugins',
  'view-machines',
  'view-account',
  'view-mobile',
  'view-metrics',
  'toggle-sidebar',
  'focus-pane-1',
  'focus-pane-2',
  'focus-pane-3',
  'focus-pane-4',
])

export function keyPassesUp(action, { view = false, focused = false } = {}) {
  return Boolean(view && focused && action && PLUGIN_KEY_ACTIONS.has(action.id))
}

// Whether a plugin's view may show here. A Global or Machine plugin's view
// shows anywhere; a project's plugin's view only when it was opened from
// that project (`openedFor`, the project it was opened in, which a stale
// hash or history entry never carries).
export function viewShown(plugin, openedFor) {
  const scope = plugin?.scope ?? 'machine'
  if (scope !== 'project' && scope !== 'project_local') return true
  return openedFor !== undefined && ridesProject(plugin, openedFor)
}

// The mark on a plugin's composer button that says whose it is: the first
// letter of its name.
export function pluginInitial(name) {
  const first = String(name ?? '').trim().match(/[\p{L}\p{N}]/u)
  return first ? first[0].toUpperCase() : '?'
}

// What a plugin's composer button says it is: the plugin's own label, and
// whose it is - every name it is read by carries the plugin's, so a
// plugin's button can't pass for one of the app's own.
export function buttonWords(input, pluginName, state) {
  const own = `${input.label} (${pluginName} plugin)`
  return {
    label: state?.label ? `${own}: ${state.label}` : own,
    title: state?.title ? `${state.title} (${pluginName} plugin)` : own,
  }
}

// Text a plugin writes going in at the cursor, or over the selection. A word
// set right after another word gets a space between them, and one before the
// word that follows, so a dictated phrase never runs into what is already
// there. Answers the new text and where the cursor goes.
export function insertAt(value, start, end, piece) {
  const current = String(value ?? '')
  const s = Math.max(0, Math.min(Number.isInteger(start) ? start : current.length, current.length))
  const e = Math.max(s, Math.min(Number.isInteger(end) ? end : s, current.length))
  const before = current.slice(0, s)
  const after = current.slice(e)
  let middle = String(piece ?? '')
  if (middle && before && !/[\s([{"'“‘]$/u.test(before) && /^[\p{L}\p{N}"'“‘(]/u.test(middle)) {
    middle = ` ${middle}`
  }
  if (middle && after && /^[\p{L}\p{N}]/u.test(after) && !/\s$/u.test(middle)) {
    middle = `${middle} `
  }
  return { text: before + middle + after, caret: before.length + middle.length }
}

// A plugin view's place in the app's navigation: the view id the history,
// the hash and the split panes carry.
export const VIEW_PREFIX = 'plugin:'

export const isPluginView = (view) => typeof view === 'string' && view.startsWith(VIEW_PREFIX)

/**
 * The project the user is in, as the Plugins tiles read it: a plugin view
 * stands in its own plugin's project, a chat in its folder's project, and
 * anything else in none. A view a plugin opens is recorded here too, never
 * in the plugin's own project, so a project's plugin cannot show its view
 * outside that project by opening it itself.
 */
export function frontProject(active, { sessions = [], projects = [], viewInfo = null } = {}) {
  if (isPluginView(active)) return viewInfo?.(active)?.plugin?.project ?? null
  return projectFor(projects, (sessions ?? []).find((s) => s.id === active)?.workspace)?.id ?? null
}

export function viewKey(pluginId, viewId) {
  return `${VIEW_PREFIX}${pluginId}/${viewId}`
}

export function parseViewKey(key) {
  if (!isPluginView(key)) return null
  const rest = key.slice(VIEW_PREFIX.length)
  const at = rest.lastIndexOf('/')
  if (at <= 0 || at === rest.length - 1) return null
  return { pluginId: rest.slice(0, at), viewId: rest.slice(at + 1) }
}

// A plugin's id as one piece of a settings search key.
export function pluginSlug(pluginId) {
  return String(pluginId).replace(/[^a-z0-9]+/gi, '-')
}

// The search key of a plugin's own block under Settings > Plugins.
export function settingsKey(pluginId) {
  return `plugin-${pluginSlug(pluginId)}`
}

// The policy every plugin frame runs under, on top of the console's own:
// scripts come only from the console's bridge and from the plugin's own
// files (turned into blob: scripts inside the frame), and the frame reaches
// no address itself - the network is openremote.fetch, made by the daemon
// for a plugin given it.
export function frameCsp(consoleOrigin) {
  return [
    "default-src 'none'",
    `script-src ${consoleOrigin} blob:`,
    "style-src 'unsafe-inline' blob: data:",
    'img-src data: blob:',
    'font-src data: blob:',
    'media-src blob: data:',
    "connect-src 'none'",
    "frame-src 'none'",
    'worker-src blob:',
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
  ].join('; ')
}

const attr = (value) => String(value).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')

// The page a plugin frame boots from: the policy above, then the bridge,
// which asks the console for the plugin's own page and builds it. It names
// nothing of the console's but its origin - no token, no daemon address.
export function frameDocument(consoleOrigin) {
  return (
    '<!doctype html><html><head><meta charset="utf-8">' +
    `<meta http-equiv="Content-Security-Policy" content="${attr(frameCsp(consoleOrigin))}">` +
    '<meta name="referrer" content="no-referrer">' +
    `<script src="${attr(consoleOrigin)}/plugin-bridge.js"></script>` +
    '</head><body></body></html>'
  )
}

// A frame may use the microphone only for a plugin given it at install. The
// frame's address can only ever be the console's blob: page (the console's
// frame-src), so the delegation reaches no other site.
export function frameAllow(powers) {
  return Array.isArray(powers) && powers.includes('microphone') ? 'microphone *' : undefined
}

// A setting's value as the form shows it: the saved one, else the plugin's
// default.
export function shownValue(setting, values) {
  const saved = values?.[setting.key]
  return saved === undefined || saved === null ? setting.default : saved
}

// What a text or number field holds: only a value the user saved. The
// daemon answers the plugin's default where none was saved, and that one
// is left out (the placeholder shows it).
// `saved`: the daemon says the user saved this value (its `saved` list).
// A default the user saved by "Use default" is theirs and shows in the
// field; the default alone stays a placeholder.
export function typedValue(setting, value, saved = false) {
  if (value == null) return ''
  if (saved) return String(value)
  if (setting.default != null && value === setting.default) return ''
  return String(value)
}

// Whether a field offers "Use default": it has a default, and that default
// is not already what the user saved. Saving it is the user's own word
// (a key sent only to a saved address goes to it then; never before).
export function offersDefault(setting, value, saved = false) {
  if (setting.default == null || setting.default === '') return false
  return !(saved && value === setting.default)
}

// A field's placeholder: the plugin's default, else its own hint.
export function fieldPlaceholder(setting) {
  if (setting.default != null && setting.default !== '') return String(setting.default)
  return setting.placeholder ?? ''
}

// The settings as a plugin reads them: every value it declares (its default
// where none was saved) and every secret as 'set' or 'unset' - whatever the
// daemon's answer leaves out. A secret's value is never among them.
export function withDefaults(view, settings) {
  const values = {}
  const secrets = {}
  for (const setting of settings ?? []) {
    if (setting.type === 'secret') secrets[setting.key] = view?.secrets?.[setting.key] === 'set' ? 'set' : 'unset'
    else values[setting.key] = shownValue(setting, view?.values ?? {})
  }
  return { values, secrets }
}

// What a field's raw input means for its setting: the value to save (null
// goes back to the plugin's default), or the words for what is wrong.
export function parseInput(setting, raw) {
  if (setting.type === 'number') {
    if (raw === '' || raw == null) return { value: null }
    const n = Number(raw)
    if (!Number.isFinite(n)) return { error: `${setting.label} takes a number.` }
    if (setting.min != null && n < setting.min) return { error: `${setting.label} is at least ${setting.min}.` }
    if (setting.max != null && n > setting.max) return { error: `${setting.label} is at most ${setting.max}.` }
    return { value: n }
  }
  if (setting.type === 'boolean') return { value: Boolean(raw) }
  if (setting.type === 'enum') {
    return setting.options.some((o) => o.value === raw)
      ? { value: raw }
      : { error: `${raw} is not one of ${setting.label}'s choices.` }
  }
  return { value: String(raw ?? '') }
}

// Bytes to base64 and back, for request and answer bodies on their way
// through the daemon's JSON. Large bodies go in slices: one huge
// String.fromCharCode call overflows the stack.
export function toBase64(bytes) {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)
  if (typeof view.toBase64 === 'function') return view.toBase64()
  let binary = ''
  for (let i = 0; i < view.length; i += 0x8000) {
    binary += String.fromCharCode.apply(null, view.subarray(i, i + 0x8000))
  }
  return btoa(binary)
}

export function fromBase64(encoded) {
  if (typeof Uint8Array.fromBase64 === 'function') return Uint8Array.fromBase64(encoded ?? '')
  const binary = atob(encoded ?? '')
  const out = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i)
  return out
}

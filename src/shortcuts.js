// Keyboard shortcuts: the action catalog, key normalization, labels, and
// the user's own bindings. Pure - no DOM, no React - so the rules are
// tested directly; keyboard.jsx wires them to the window.
//
// A combo is a string like `Mod+Shift+A`. `Mod` is the platform's primary
// modifier - Ctrl on Windows and Linux, Cmd on macOS - so one default map
// serves every desktop. `Ctrl` in a combo is only ever the literal
// Control key on macOS (on the others it is `Mod`).

const PREFS_KEY = 'openremote-shortcuts'

// Every action that can carry a key, in the order the cheat sheet and the
// Keyboard settings list them. `keys` is the default (null: unbound by
// default, reachable from the palette and bindable).
export const GROUPS = [
  { id: 'navigation', label: 'Navigation' },
  { id: 'chat', label: 'Chat' },
  { id: 'pickers', label: 'Pickers' },
  { id: 'layout', label: 'Layout' },
  { id: 'browser', label: 'Browser' },
]

const modelSwitches = Array.from({ length: 9 }, (_, i) => ({
  id: `model-${i + 1}`,
  group: 'pickers',
  label: `Switch to model ${i + 1}`,
  keys: `Mod+Alt+${i + 1}`,
}))

export const ACTIONS = [
  // `navigates: true` marks an action that leaves a chat: it can unmount the
  // composer, and a draft lives in that view (not in the shell), so firing it
  // from inside a field would lose text the dispatcher cannot see. The rule is
  // here, in the catalog, rather than in each handler: see blocksWhileTyping.
  { id: 'new-chat', group: 'navigation', label: 'New chat', keys: 'Mod+N', navigates: true },
  { id: 'new-chat-in-project', group: 'navigation', label: 'New chat in this project', keys: 'Mod+Shift+N', navigates: true },
  { id: 'palette', group: 'navigation', label: 'Command palette', keys: 'Mod+K' },
  { id: 'cheat-sheet', group: 'navigation', label: 'Keyboard shortcuts', keys: 'Mod+/' },
  { id: 'search-chats', group: 'navigation', label: 'Search chats', keys: 'Mod+F' },
  { id: 'next-chat', group: 'navigation', label: 'Next chat', keys: 'Alt+Down' },
  { id: 'prev-chat', group: 'navigation', label: 'Previous chat', keys: 'Alt+Up' },
  { id: 'first-chat', group: 'navigation', label: 'First chat', keys: 'Alt+Home' },
  { id: 'last-chat', group: 'navigation', label: 'Last chat', keys: 'Alt+End' },
  { id: 'view-settings', group: 'navigation', label: 'Settings', keys: 'Mod+,', navigates: true },
  { id: 'view-automations', group: 'navigation', label: 'Automations', keys: null, navigates: true },
  { id: 'view-plugins', group: 'navigation', label: 'Plugins', keys: null, navigates: true },
  { id: 'view-machines', group: 'navigation', label: 'Machines', keys: null, navigates: true },
  { id: 'view-account', group: 'navigation', label: 'Account', keys: null, navigates: true },
  { id: 'view-mobile', group: 'navigation', label: 'Mobile', keys: null, navigates: true },
  { id: 'view-metrics', group: 'navigation', label: 'Metrics', keys: null, navigates: true },
  { id: 'focus-composer', group: 'chat', label: 'Focus the composer', keys: 'Mod+L' },
  { id: 'steer', group: 'chat', label: 'Steer', keys: 'Mod+Enter' },
  { id: 'interrupt', group: 'chat', label: 'Interrupt the turn', keys: 'Esc' },
  { id: 'stop', group: 'chat', label: 'Stop the session', keys: 'Mod+Shift+X' },
  { id: 'archive', group: 'chat', label: 'Archive the chat', keys: 'Mod+Shift+A', navigates: true },
  { id: 'pick-model', group: 'pickers', label: 'Model', keys: 'Mod+M' },
  { id: 'pick-effort', group: 'pickers', label: 'Effort', keys: 'Mod+E' },
  { id: 'pick-permission', group: 'pickers', label: 'Permission', keys: 'Mod+P' },
  { id: 'pick-harness', group: 'pickers', label: 'App (new chat)', keys: 'Mod+H' },
  { id: 'toggle-fast', group: 'pickers', label: 'Fast on or off', keys: 'Mod+Shift+F' },
  ...modelSwitches,
  { id: 'toggle-sidebar', group: 'layout', label: 'Show or hide the sidebar', keys: 'Mod+B' },
  // The browser's pane: beside the chat, the pages an agent can drive.
  { id: 'toggle-browser', group: 'layout', label: 'Show or hide the browser', keys: 'Mod+Shift+B' },
  { id: 'toggle-mode', group: 'layout', label: 'Switch Local and Cloud', keys: 'Mod+Shift+M', navigates: true },
  { id: 'split-right', group: 'layout', label: 'Split right', keys: 'Mod+\\' },
  { id: 'split-down', group: 'layout', label: 'Split down', keys: 'Mod+Shift+\\' },
  // The panes in reading order, the way VS Code numbers editor groups.
  { id: 'focus-pane-1', group: 'layout', label: 'Focus the first pane', keys: 'Mod+1' },
  { id: 'focus-pane-2', group: 'layout', label: 'Focus the second pane', keys: 'Mod+2' },
  { id: 'focus-pane-3', group: 'layout', label: 'Focus the third pane', keys: 'Mod+3' },
  { id: 'focus-pane-4', group: 'layout', label: 'Focus the fourth pane', keys: 'Mod+4' },
  { id: 'close-pane', group: 'layout', label: 'Close the pane', keys: 'Mod+W', navigates: true },
]

// The built-in browser's keys: Chrome's, acting only while the browser has
// focus (its tab strip, toolbar, address bar or page). They live in a scope
// of their own, so Ctrl+W closes a tab in the browser and the pane
// everywhere else; taking a key moves it only between actions of one scope.
// A page hands the console only the chords the shell takes for it (TAKEN in
// src-tauri/src/browser/win.rs), so every default here is one of those.
// Ctrl+Shift+B stays the app's (show or hide the browser) even from a page.
export const BROWSER_ACTIONS = [
  { id: 'browser-new-tab', group: 'browser', label: 'New tab', keys: 'Mod+T' },
  { id: 'browser-close-tab', group: 'browser', label: 'Close the tab', keys: 'Mod+W' },
  { id: 'browser-reopen-tab', group: 'browser', label: 'Reopen the closed tab', keys: 'Mod+Shift+T' },
  { id: 'browser-next-tab', group: 'browser', label: 'Next tab', keys: 'Mod+Tab' },
  { id: 'browser-prev-tab', group: 'browser', label: 'Previous tab', keys: 'Mod+Shift+Tab' },
  { id: 'browser-address', group: 'browser', label: 'Go to the address bar', keys: 'Mod+L' },
  { id: 'browser-back', group: 'browser', label: 'Back', keys: 'Alt+Left' },
  { id: 'browser-forward', group: 'browser', label: 'Forward', keys: 'Alt+Right' },
  { id: 'browser-reload', group: 'browser', label: 'Reload', keys: 'Mod+R' },
  { id: 'browser-reload-key', group: 'browser', label: 'Reload, second key', keys: 'F5' },
  { id: 'browser-find', group: 'browser', label: 'Find in page', keys: 'Mod+F' },
  { id: 'browser-bookmark', group: 'browser', label: 'Bookmark this tab', keys: 'Mod+D' },
  { id: 'browser-history', group: 'browser', label: 'History', keys: 'Mod+H' },
  { id: 'browser-downloads', group: 'browser', label: 'Downloads', keys: 'Mod+J' },
  { id: 'browser-zoom-in', group: 'browser', label: 'Zoom in', keys: 'Mod+=' },
  { id: 'browser-zoom-out', group: 'browser', label: 'Zoom out', keys: 'Mod+-' },
  { id: 'browser-zoom-reset', group: 'browser', label: 'Actual size', keys: 'Mod+0' },
  { id: 'browser-devtools', group: 'browser', label: 'Developer tools', keys: 'F12' },
]

// Every action, app and browser, in the order Settings and the cheat sheet
// list them.
export const CATALOG = [...ACTIONS, ...BROWSER_ACTIONS]

// The actions listed where this console runs: the browser's only where
// there is a browser (the desktop app).
export function shownActions(hasBrowser) {
  return hasBrowser ? CATALOG : ACTIONS
}

// macOS keeps Cmd+H (hide), Cmd+M (minimize) and Cmd+W (close the
// window) for itself, so those actions default to Option+Cmd there. The
// browser's keys follow Chrome for the Mac where Cmd+Tab (switching apps)
// or Option+arrows (moving by word) would clash.
const MAC_KEYS = {
  'pick-harness': 'Mod+Alt+H',
  'pick-model': 'Mod+Alt+M',
  'close-pane': 'Mod+Alt+W',
  'browser-close-tab': 'Mod+Alt+W',
  'browser-next-tab': 'Ctrl+Tab',
  'browser-prev-tab': 'Ctrl+Shift+Tab',
  'browser-back': 'Mod+[',
  'browser-forward': 'Mod+]',
  'browser-history': 'Mod+Y',
  'browser-downloads': 'Mod+Shift+J',
}

const ACTION_IDS = new Set(CATALOG.map((a) => a.id))
const BROWSER_IDS = new Set(BROWSER_ACTIONS.map((a) => a.id))

function scopeOf(id) {
  return BROWSER_IDS.has(id) ? BROWSER_ACTIONS : ACTIONS
}

// The default key for an action on this platform.
export function defaultKey(id, mac = false) {
  if (mac && Object.hasOwn(MAC_KEYS, id)) return MAC_KEYS[id]
  return CATALOG.find((a) => a.id === id)?.keys ?? null
}

const MOD_ORDER = ['Mod', 'Ctrl', 'Alt', 'Shift']

// e.code, not e.key: the physical key, so a binding survives keyboard
// layouts and Shift never turns `\` into `|` or `1` into `!`.
const CODE_KEYS = {
  Backslash: '\\',
  IntlBackslash: '\\',
  Slash: '/',
  Comma: ',',
  Period: '.',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Minus: '-',
  Equal: '=',
  Backquote: '`',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  Escape: 'Esc',
  Enter: 'Enter',
  NumpadEnter: 'Enter',
  Space: 'Space',
  Tab: 'Tab',
  Backspace: 'Backspace',
  Delete: 'Delete',
}

const MODIFIER_CODES = /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/

function keyName(code, key) {
  if (!code) {
    // No physical code (some virtual keyboards): the printed character.
    return typeof key === 'string' && key.length === 1 ? key.toUpperCase() : null
  }
  if (MODIFIER_CODES.test(code)) return null
  let m = /^Key([A-Z])$/.exec(code)
  if (m) return m[1]
  m = /^(?:Digit|Numpad)(\d)$/.exec(code)
  if (m) return m[1]
  if (/^F([1-9]|1[0-2])$/.test(code)) return code
  return CODE_KEYS[code] ?? null
}

// The combo a keydown event spells, or null for a lone modifier, an
// unknown key, or the Windows key on Windows/Linux (never ours to take).
export function comboFromEvent(e, mac) {
  if (!mac && e.metaKey) return null
  const key = keyName(e.code, e.key)
  if (!key) return null
  const mods = {
    Mod: mac ? e.metaKey : e.ctrlKey,
    Ctrl: mac ? e.ctrlKey : false,
    Alt: e.altKey,
    Shift: e.shiftKey,
  }
  return [...MOD_ORDER.filter((m) => mods[m]), key].join('+')
}

const COMBO_RE = /^((Mod|Ctrl|Alt|Shift)\+)*[^+]+$/

function validCombo(combo) {
  return typeof combo === 'string' && combo.length > 0 && COMBO_RE.test(combo)
}

const KEY_GLYPHS = { Up: '↑', Down: '↓', Left: '←', Right: '→' }
const MAC_MODS = { Mod: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧' }

function splitCombo(combo) {
  // Modifiers lead; the key is last (no key name contains a `+`).
  const parts = combo.split('+')
  const key = parts.pop()
  return { mods: parts, key }
}

// What a combo looks like on this platform: `Ctrl+Shift+A` on Windows and
// Linux, `⌘⇧A` on macOS.
export function comboLabel(combo, mac) {
  if (!combo) return ''
  const { mods, key } = splitCombo(combo)
  const k = KEY_GLYPHS[key] ?? key
  if (mac) return mods.map((m) => MAC_MODS[m] ?? m).join('') + k
  return [...mods.map((m) => (m === 'Mod' ? 'Ctrl' : m)), k].join('+')
}

// Whether a combo may fire while the user is typing in a field. A bare
// key (or Shift+key) would eat the character the user meant to type, so
// only combos with Ctrl/Cmd/Alt, Esc, or a function key pass.
export function firesWhileTyping(combo) {
  const { mods, key } = splitCombo(combo)
  return mods.some((m) => m === 'Mod' || m === 'Ctrl' || m === 'Alt') || key === 'Esc' || /^F\d+$/.test(key)
}

/// Whether a focused element is a text field, so the dispatcher can tell
/// "typing" from "looking at the app". Element-shaped rather than DOM-bound:
/// the keyboard layer hands it `document.activeElement`, and the same test is
/// used with plain objects.
export function typesIn(el) {
  if (!el) return false
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable === true
}

/// Whether an action must stand down while the caret is in a text field. Only
/// the actions that leave a chat do: a draft lives in the composer's own view,
/// not in the shell, so navigation fired from inside a field unmounts the text
/// with nothing able to save it first. Steering, stopping, switching panes,
/// the palette and the cheat sheet all keep the composer mounted, so they are
/// unaffected and keep working while the user types.
export function blocksWhileTyping(actionId) {
  const action = ACTIONS.find((a) => a.id === actionId)
  return Boolean(action?.navigates)
}

/// Why a rebind capture must not take `combo`, or null when it may. A key with
/// no Ctrl, Alt or function modifier never reaches the dispatcher while a text
/// field has focus, so storing one would draw a key on this page that never
/// runs where the user writes. Esc and the function keys are the exceptions
/// the dispatcher already delivers while typing. The sentence names the field
/// rather than the character, because some of these keys (Delete, Tab, Home)
/// are never typed at all: the field takes them and OpenRemote does not.
export function captureRefusal(combo) {
  if (!validCombo(combo)) return 'That is not a key combination OpenRemote can use.'
  if (firesWhileTyping(combo)) return null
  return 'A key with no modifier goes to the field you are typing in, not to OpenRemote, so it cannot be a shortcut. Add Ctrl, Alt or a function key.'
}

// The user's changes to the default map: { actionId: combo | null }, null
// meaning deliberately unbound. Unknown actions and malformed combos are
// dropped, never trusted.
export function loadOverrides() {
  let raw = {}
  try {
    raw = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}')
  } catch {
    raw = {}
  }
  const out = {}
  if (raw && typeof raw === 'object') {
    for (const [id, combo] of Object.entries(raw)) {
      // An id this build no longer knows is kept, not dropped: it is the
      // user's saved key, and the settings page shows it as a row that can be
      // cleared rather than making it vanish on the next launch.
      if (combo === null || validCombo(combo)) out[id] = combo
    }
  }
  return out
}

/// The saved bindings whose action this build no longer has. The settings page
/// shows them as one row; nothing else reads them (`effectiveBindings` walks
/// the catalog, so an unknown id can never take a key).
export function staleOverrides(overrides) {
  return Object.entries(overrides ?? {})
    .filter(([id]) => !ACTION_IDS.has(id))
    .map(([id, combo]) => ({ id, combo }))
    .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
}

/// Forget the saved bindings whose action is gone, and nothing else.
export function clearStaleOverrides(overrides) {
  const out = {}
  for (const [id, combo] of Object.entries(overrides ?? {})) {
    if (ACTION_IDS.has(id)) out[id] = combo
  }
  return out
}

export function saveOverrides(overrides) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(overrides))
  } catch {
    /* storage unavailable */
  }
}

// Every action's live combo within one scope: the user's choice where they
// made one, else the default - but a default never shares a key with a
// user's choice (the user's choice wins; the default is shadowed to
// unbound).
function scopeBindings(list, overrides, mac) {
  const out = {}
  const taken = new Set()
  for (const a of list) {
    if (Object.hasOwn(overrides, a.id)) {
      out[a.id] = overrides[a.id]
      if (overrides[a.id]) taken.add(overrides[a.id])
    }
  }
  for (const a of list) {
    if (Object.hasOwn(out, a.id)) continue
    const key = defaultKey(a.id, mac)
    out[a.id] = key && !taken.has(key) ? key : null
    if (out[a.id]) taken.add(out[a.id])
  }
  return out
}

// The app's own actions and their live keys: what the window listener
// dispatches.
export function effectiveBindings(overrides, mac = false) {
  return scopeBindings(ACTIONS, overrides, mac)
}

// The browser's actions and their live keys: the browser dispatches these
// itself, while it has focus.
export function browserBindings(overrides, mac = false) {
  return scopeBindings(BROWSER_ACTIONS, overrides, mac)
}

export function actionForCombo(bindings, combo) {
  for (const [id, c] of Object.entries(bindings)) if (c === combo) return id
  return null
}

// Taking a key another action of the same scope holds moves it: the new
// action gets the key, the other is left unbound. Returns the new overrides
// and the actions that lost their key (for the "moved from" message).
export function rebind(overrides, actionId, combo, mac = false) {
  const before = scopeBindings(scopeOf(actionId), overrides, mac)
  const displaced = Object.keys(before).filter((id) => id !== actionId && before[id] === combo)
  const next = { ...overrides, [actionId]: combo }
  for (const id of displaced) next[id] = null
  return { overrides: next, displaced }
}

export function unbind(overrides, actionId) {
  return { ...overrides, [actionId]: null }
}

// Back to the default key. If another action of its scope took that key
// meanwhile, it gives it back and is left unbound - the same rule as a
// rebind.
export function resetOne(overrides, actionId, mac = false) {
  const next = { ...overrides }
  delete next[actionId]
  const key = defaultKey(actionId, mac)
  if (!key) return next
  const live = scopeBindings(scopeOf(actionId), next, mac)
  if (live[actionId] === key) return next
  for (const [id, combo] of Object.entries(live)) {
    if (id !== actionId && combo === key) next[id] = null
  }
  return next
}

export function isOverridden(overrides, actionId) {
  return Object.hasOwn(overrides, actionId)
}

// The palette's ranking: a whole substring beats a scattered match, a
// match at a word's start beats one inside it, and consecutive letters
// beat gaps. Null when the text doesn't contain the query's letters in
// order at all.
export function fuzzyScore(query, text) {
  const q = query.trim().toLowerCase()
  if (!q) return 0
  const t = String(text ?? '').toLowerCase()
  const at = t.indexOf(q)
  if (at !== -1) {
    const wordStart = at === 0 || /[\s\-_/.\\]/.test(t[at - 1])
    return 1000 + (wordStart ? 200 : 0) - at - (t.length - q.length) * 0.1
  }
  let from = 0
  let prev = -2
  let run = 0
  let score = 0
  for (const ch of q) {
    if (ch === ' ') continue
    const found = t.indexOf(ch, from)
    if (found === -1) return null
    const wordStart = found === 0 || /[\s\-_/.\\]/.test(t[found - 1])
    run = found === prev + 1 ? run + 1 : 0
    score += 10 + run * 5 + (wordStart ? 15 : 0) - Math.min(found - from, 10)
    prev = found
    from = found + 1
  }
  return score
}

// Items that match, best first. Two items that score the same are settled by
// `tie` when the caller has a rule for it (the palette prefers a label the
// query begins, then a chat over an action); with no rule, and whenever the
// rule says nothing, they keep the order they came in. The score is the only
// thing that ranks: a rule can never lift an item over a better score.
export function rankMatches(query, items, textOf, tie = null) {
  return items
    .map((item, i) => ({ item, i, score: fuzzyScore(query, textOf(item)) }))
    .filter((r) => r.score !== null)
    .sort((a, b) => b.score - a.score || (tie ? tie(a.item, b.item) : 0) || a.i - b.i)
    .map((r) => r.item)
}

/**
 * The rule `rankMatches` uses to settle a tie, for the palette: a label the
 * query begins beats one that only contains it, and then the caller's
 * preferred kind (the palette passes "is a chat") beats the rest. Everything
 * else keeps the order the rows came in, which is what keeps every ranking
 * that already differed exactly as it was: the rule is asked only when the
 * scores match, and a null answer means "no opinion".
 */
export function tieRule(query, isPreferred = null) {
  const prefix = String(query ?? '').trim().toLowerCase()
  if (!prefix) return null
  const starts = (row) => typeof row?.name === 'string' && row.name.toLowerCase().startsWith(prefix)
  return (a, b) => {
    if (starts(a) !== starts(b)) return starts(a) ? -1 : 1
    if (Boolean(isPreferred?.(a)) !== Boolean(isPreferred?.(b))) return isPreferred(a) ? -1 : 1
    return 0
  }
}

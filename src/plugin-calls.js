// The console's answers to a plugin frame's calls, kept free of React so the
// rules are tested directly. plugins-host.jsx holds the frames and hands
// each call here; this module decides whether it is answered and with what.
//
// Three rules hold for every answer:
// - the frame gets only the call's own result: the console makes the
//   daemon request itself (the token rides there and nowhere else), and
//   hands back what the route answered, never the request it made;
// - a call is answered only when the plugin's install record allows it
//   (refusal() in plugin-runtime.js), and the chat calls go to the daemon,
//   which checks the record again and keeps a project's plugin inside its
//   project;
// - a plugin writes in a composer only after the user clicked its button
//   in that composer: a click is the grant, and nothing else makes one.

import {
  CHAT_CALLS,
  fromBase64,
  insidePath,
  refusal,
  toBase64,
  viewKey,
  withDefaults,
} from './plugin-runtime.js'

export const MAX_INSERT = 100_000
export const MAX_ATTACH_BYTES = 10 * 1024 * 1024
export const INPUT_STATES = new Set(['idle', 'active', 'busy'])

// Which composers each plugin may write in. `composers` maps a composer's
// handle to { insert, attachKey }; `grants` maps a plugin to the handles
// the user clicked its button in (each with its composer's key, so a chat's
// composer that opens again still counts); `lastClick` is the handle of the
// plugin's last click.
export function composerGrants() {
  return { composers: new Map(), grants: new Map(), lastClick: new Map() }
}

// The user clicked a plugin's button in the composer `handle`: the one way
// a grant is made.
export function grantClick(state, pluginId, handle) {
  const granted = state.grants.get(pluginId) ?? new Map()
  granted.set(handle, state.composers.get(handle)?.attachKey ?? null)
  state.grants.set(pluginId, granted)
  state.lastClick.set(pluginId, handle)
}

// A plugin turned off or removed loses its grants.
export function dropGrants(state, livePluginIds) {
  for (const id of [...state.grants.keys()]) if (!livePluginIds.has(id)) state.grants.delete(id)
  for (const id of [...state.lastClick.keys()]) if (!livePluginIds.has(id)) state.lastClick.delete(id)
}

// The composer a plugin writes in: the one it names (a handle a click gave
// it), else the one whose button the user clicked last. Never a composer the
// user didn't point it at - though when the user left that chat and came
// back, its composer is a new one, and the words still belong there.
export function composerFor(state, pluginId, handle) {
  const granted = state.grants.get(pluginId)
  const chosen = handle ?? state.lastClick.get(pluginId) ?? null
  if (!chosen || !granted?.has(chosen)) {
    throw new Error('A plugin writes in a composer only after the user clicks its button there.')
  }
  const target = state.composers.get(chosen)
  if (target) return target
  const key = granted.get(chosen)
  const again = key == null ? null : [...state.composers.values()].find((c) => c.attachKey === key)
  if (again) return again
  throw new Error('That composer has closed.')
}

// The settings answer, whichever shape the daemon gives it.
export const settingsView = (reply) => {
  const view = reply?.result ?? reply ?? {}
  return { values: view.values ?? {}, secrets: view.secrets ?? {} }
}

// The calls a frame may make. Each takes the plugin's record (`{ plugin,
// ui, name }`) and the call's params, and answers [result, transfer?].
// `deps` is what the console lends: its api, a note in the app, opening a
// view or Settings, a composer button's state, an attachment, and the
// composer grants.
export function makeCalls(deps) {
  const chat = (method) => async ({ plugin }, params) => {
    const answer = await deps.api().pluginCall(plugin.id, CHAT_CALLS[method], chatParams(method, params))
    return [answer?.result ?? null]
  }
  return {
    'settings.get': async ({ plugin, ui }) => [
      withDefaults(settingsView(await deps.api().pluginSettings(plugin.id)), ui.settings),
    ],

    'files.read': async ({ plugin }, { path }) => {
      if (!insidePath(path)) throw new Error(`${path} is not a path inside the plugin's folder`)
      const file = await deps.api().pluginFile(plugin.id, path)
      return [{ type: file.type, bytes: file.bytes }, [file.bytes]]
    },

    'ui.toast': async ({ name }, { text }) => {
      const words = String(text ?? '').trim()
      if (words) deps.toast(name, words.slice(0, 280))
      return [null]
    },

    'ui.openView': async ({ plugin, ui }, { view }) => {
      const target = view ? ui.views.find((v) => v.id === view) : ui.views[0]
      if (!target) throw new Error(view ? `There is no view named ${view}.` : 'This plugin has no view.')
      deps.openView(viewKey(plugin.id, target.id))
      return [null]
    },

    'ui.openSettings': async ({ plugin, ui }) => {
      if (!ui.settings.length) throw new Error('This plugin has no settings.')
      deps.openSettings(plugin.id)
      return [null]
    },

    'composer.setState': async ({ plugin, ui }, { input, state, label, title, composer }) => {
      if (!ui.inputs.some((i) => i.id === input)) throw new Error(`There is no composer button named ${input}.`)
      const granted = deps.grants.grants.get(plugin.id)
      const scoped = Boolean(composer && granted?.has(composer))
      deps.setInputState(`${plugin.id}/${input}`, {
        state: INPUT_STATES.has(state) ? state : 'idle',
        label: typeof label === 'string' && label.trim() ? label.trim().slice(0, 40) : null,
        title: typeof title === 'string' && title.trim() ? title.trim().slice(0, 120) : null,
        // One composer's button only, when the plugin names one it was
        // handed by a click; every composer's otherwise.
        composer: scoped ? composer : null,
        composerKey: scoped ? (granted.get(composer) ?? null) : null,
      })
      return [null]
    },

    'composer.insert': async ({ plugin }, { text, composer }) => {
      const target = composerFor(deps.grants, plugin.id, composer)
      target.insert(String(text ?? '').slice(0, MAX_INSERT))
      return [null]
    },

    'composer.attach': async ({ plugin }, { name, mime, bytes, composer }) => {
      const target = composerFor(deps.grants, plugin.id, composer)
      if (!/^image\/(png|jpeg|gif|webp)$/i.test(mime ?? '')) {
        throw new Error('Only pictures (PNG, JPEG, GIF, WebP) can be attached for now.')
      }
      if (!(bytes instanceof ArrayBuffer) || bytes.byteLength > MAX_ATTACH_BYTES) {
        throw new Error('A picture can be up to 10 MB.')
      }
      deps.attach(
        { kind: 'image', name: String(name ?? 'picture').slice(0, 120), mime, dataUrl: `data:${mime};base64,${toBase64(bytes)}` },
        target.attachKey,
      )
      return [null]
    },

    'chats.list': chat('chats.list'),
    'chats.read': chat('chats.read'),
    'chats.start': chat('chats.start'),
    'chats.steer': chat('chats.steer'),
    'chats.stop': chat('chats.stop'),

    fetch: async ({ plugin }, { url, method, headers, body, timeoutMs }) => {
      const reply = await deps.api().pluginFetch(plugin.id, {
        url: String(url ?? ''),
        method: String(method ?? 'GET'),
        headers: Array.isArray(headers)
          ? headers.filter((h) => Array.isArray(h) && h.length === 2).map(([n, v]) => [String(n), String(v)])
          : [],
        body: body instanceof ArrayBuffer && body.byteLength ? toBase64(body) : null,
        ...(Number.isFinite(timeoutMs) ? { timeout_ms: timeoutMs } : {}),
      })
      const bytes = fromBase64(reply?.body ?? '')
      const answer =
        bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes.buffer : bytes.slice().buffer
      return [{ status: reply?.status ?? 502, headers: reply?.headers ?? [], body: answer }, [answer]]
    },
  }
}

// The frame's words for a chat call, in the daemon's shape: the chat by
// its id, and only the fields the method reads.
function chatParams(method, params = {}) {
  const id = params.id == null ? undefined : String(params.id)
  switch (method) {
    case 'chats.read':
    case 'chats.stop':
      return { chat: id }
    case 'chats.steer':
      return { chat: id, text: String(params.text ?? '') }
    case 'chats.start':
      return {
        harness: params.harness ?? undefined,
        workspace: params.workspace ?? undefined,
        model: params.model ?? undefined,
        prompt: typeof params.text === 'string' && params.text.trim() ? params.text : undefined,
      }
    default:
      return {}
  }
}

// One call answered: refused in plain words when the record does not allow
// it, else the call's own result. Answers { result, transfer } or { error }.
export async function answerCall(record, message, calls) {
  if (!record || !record.plugin.enabled) return { error: 'This plugin is turned off.' }
  const method = String(message.method ?? '')
  const refused = refusal(method, {
    name: record.name,
    powers: record.plugin.powers ?? [],
    inputs: record.ui.inputs,
  })
  if (refused) return { error: refused }
  const handler = Object.hasOwn(calls, method) ? calls[method] : null
  if (!handler) return { error: `There is no openremote call named ${method}.` }
  try {
    const params = message.params && typeof message.params === 'object' ? message.params : {}
    const [result, transfer] = await handler(record, params)
    return { result: result ?? null, transfer }
  } catch (err) {
    return { error: String(err?.message ?? err) }
  }
}

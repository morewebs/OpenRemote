# JavaScript plugins: the frame and `window.openremote`

A JavaScript plugin runs inside the OpenRemote console, in a frame of its
own. It can add a button to every composer, open views in a pane, keep
settings, and, with the powers the user approved when installing it, read
chats, start and steer them, reach the network and use the microphone.
This page is the plugin author's reference. The dictation plugin in
`examples/plugins/dictation` is a complete, small example.

What a plugin may do is read from its **install record**: the powers and
contributions the user approved on the install screen. The console and
the daemon both check every call against that record, never against the
plugin's files as they are now, so editing an installed plugin's
`openremote.json` changes nothing it may do.

## The folder

A plugin folder is Claude Code's plugin layout plus OpenRemote's own
`openremote.json` beside it, so one folder is a plugin to both:

```
my-plugin/
  .claude-plugin/plugin.json   Claude Code's manifest (name, version, ...)
  openremote.json              what OpenRemote runs, and what it may do
  main.js                      the page (or index.html)
```

`openremote.json`:

```json
{
  "api": 1,
  "name": "my-plugin",
  "version": "0.1.0",
  "description": "One line the install review shows.",
  "runtime": "js",
  "entry": "main.js",
  "powers": ["network"],
  "contributes": {
    "composer_inputs": [{ "id": "go", "label": "Do the thing", "icon": "sparkle" }],
    "views": [{ "id": "main", "title": "My plugin", "icon": "notebook", "place": "sidebar", "entry": "view.html" }],
    "settings": [
      { "key": "base_url", "type": "string", "label": "Address", "help": "Where it goes.", "default": "https://example.com/v1" },
      { "key": "api_key", "type": "secret", "label": "API key", "sendTo": "base_url" }
    ]
  }
}
```

This is the same `openremote.json` every plugin runtime reads (see
[authoring.md](authoring.md)); a JavaScript plugin uses the
`composer_inputs`, `views` and `settings` parts of it.

| Field | Meaning |
|---|---|
| `entry` | The page, a path inside the folder: a `.js` (or `.mjs`) file, or an `.html` page. |
| `powers` | Any of `read_chats`, `agent_tools`, `network`, `microphone`. The user approves them at install; a call that needs one the record doesn't hold is refused. |
| `contributes.composer_inputs` | Buttons for the composer's foot: `id` (lowercase letters, digits, dashes), `label` (up to 40 characters), `icon`. The plugin's own `entry` answers them unless a button names an `entry` of its own. |
| `contributes.views` | Views, each a tile in the sidebar's Plugins group that opens in a pane: `id`, `title`, `icon`, `place` (`sidebar`), and the page's `entry`. Views placed in `machines` or `settings` are accepted but have no place in the console yet. |
| `contributes.settings` | The plugin's settings, in order; see below. |

People read the plugin's name with its first letter raised and dashes as
spaces (`voice-notes` reads "Voice notes").

There is no background page: a plugin with composer buttons costs nothing
until the user first clicks one of its buttons, which starts its hidden
page and hands it that click.

Icons are named in letters only, case and dashes aside, from the app's
own set: `microphone`, `waveform`, `translate`, `sparkle`, `magicwand`,
`notepencil`, `notebook`, `bookopen`, `listchecks`, `chatcircle`,
`camera`, `image`, `paperclip`, `globe`, `code`, `terminal`, `robot`,
`lightning`, `clock`, `calendar`, `chartbar`, `puzzlepiece`
(`MagicWand` and `magicwand` are the same icon). Any other name shows the
puzzle piece.

### Settings

Each setting is an object with:

| Field | Meaning |
|---|---|
| `key` | Lowercase letters, digits and underscores; unique. |
| `type` | `string`, `secret`, `boolean`, `number` or `enum`. |
| `label`, `help` | What the form shows. |
| `default` | The value until the user sets one. A text or number field shows it as its placeholder and holds only what the user typed; emptying the field goes back to the default. Beside the field, **Use default** saves the default as the user's own value, by their click. |
| `min`, `max` | A number's bounds. |
| `options` | An enum's choices: strings, or `{ "value": "...", "label": "..." }`. |
| `required` | Marked as needed. OpenRemote enforces it for process and WASM plugins (they don't run until it is set); a JavaScript plugin checks its own settings. |
| `sendTo` | A secret only (`send_to` reads too): the `key` of a `string` setting holding an address. The secret is then written only into requests to the address **the user saved** there (the same scheme, host and port, and a path at or under it); a request elsewhere that names it is refused. The setting's default never counts: until the user saves an address (typing one, or clicking Use default), the header that names the secret is left out, or, when the secret is `required`, the request is refused with a sentence saying which address to save. |

The form is Settings > Plugins > your plugin's name, and the Installed
tab's Settings button opens it. A secret is write-only: once saved, no
one reads it back (not the form, not the console, not your plugin). Your
plugin uses it by naming it in a request header (see `openremote.fetch`).

## The frame

Every page of your plugin runs in a frame sandboxed to scripts alone
(`sandbox="allow-scripts"`):

- It has no origin of its own (it is opaque), so it can read nothing of
  the app, and `localStorage`, cookies and IndexedDB are not there. Keep
  what must last in your settings.
- It never holds the daemon's token or address. Every call it makes goes
  to the console by `postMessage`, and the console makes the daemon
  request itself and hands back only the answer.
- It reaches no address itself: the frame's policy refuses `fetch`,
  `XMLHttpRequest`, `WebSocket`, other frames, and remote scripts, styles
  and pictures. The network is `openremote.fetch`, for a plugin given it.
- No pop-ups, no new windows, no `alert` or `confirm`, no downloads, no
  forms that submit.
- The microphone is delegated to the frame only for a plugin given that
  power.

The console builds your page from your folder:

- A `.js` entry runs as the page's one script, in an empty page.
- A page that can't be built says why: in its view, or, for the hidden
  page behind composer buttons, in a note in the app.
- An `.html` entry has its markup and inline styles kept; its linked
  stylesheets (`<link rel="stylesheet">`) read in; its pictures and media
  (`src`, `poster`) read in as `data:` URLs; then its scripts run in
  order, inline or from your folder. Remote files are left out, and so
  are the files a stylesheet's own `url()` names: put those in as
  `data:` URLs.
- Scripts run after the markup is in place; `DOMContentLoaded` and
  `load` are sent once they have run.
- Inline event handlers (`onclick="..."`), `eval` and `javascript:`
  links don't run: use `addEventListener`.
- A module script runs, but cannot `import` your other files: bundle
  modules into one file, or use classic scripts.

The page wears the app's look: its colors, type and spacing are CSS
variables of the app's own names (`--bg-app`, `--bg-side`, `--bg-hover`,
`--bg-active`, `--line`, `--text-1`, `--text-2`, `--text-3`, `--accent`,
`--green`, `--dot-idle`, `--radius`, `--font-sans`), set on the page's
root, and the body already uses them. When the user turns on Reduce
motion, the page's `<html>` carries `data-reduce-motion`: turn your
animations off under it and under
`@media (prefers-reduced-motion: reduce)`.

### Keys

While one of your views has the focus, a key pressed with Ctrl, Cmd or
Alt that your page did not handle (`preventDefault`), outside its own
fields, is offered to the app. The app acts on it only if it is one of an
explicit list of keys that move around the app: new chat, the command
palette, the shortcut sheet, chat search, the next, previous, first and
last chat, the main views (Settings, Automations, Plugins, Machines,
Account, Mobile, Metrics), the sidebar toggle, and focusing pane 1 to 4.
Anything that acts on a chat (steer, stop, archive, interrupt), a picker,
the browser, closing a pane, switching modes or the split keys is never
passed, and nothing is passed from a hidden page or from a view without
the focus. A key the app adds later passes only once it joins that list.

## `window.openremote`

Every call returns a promise. A refused call (a power not approved, a
composer the user didn't click in) rejects with an `Error` whose message
says why in plain words, ready to show. The calls are a closed list: a
name not on this page is refused before anything is asked.

### Where the page runs

| | |
|---|---|
| `openremote.version` | `1` |
| `openremote.plugin` | `{ id, name, version, powers }`, the powers from the install record. |
| `openremote.place` | `'background'` (the hidden page behind composer buttons) or `'view'` |
| `openremote.view` | The view's `id`, in a view |

### The composer (a plugin with composer buttons)

A plugin writes in a composer only after **the user clicks the plugin's
button in that composer**. The click is the grant, and nothing else makes
one. A grant ends at the first of these:

- the plugin sets that button back to `'idle'` (its work is done);
- the composer closes, or now shows another chat;
- five minutes have passed since the click;
- the plugin is turned off or removed.

After that, writing needs a new click. Nothing a plugin does ever sends a
message.

Every button of yours carries a small badge with your plugin's initial,
and its name and tooltip read "Label (Name plugin)", so it can't pass for
one of the app's own.

| Call | Does |
|---|---|
| `composer.onClick(handler)` | `handler({ input, composer })` on each click of one of your buttons. `input` is the button's `id`; `composer` is a handle to the composer it was clicked in. Answers a function that stops listening. Clicks that came before your page was up are kept for your first handler. |
| `composer.insert(text, { composer })` | Writes `text` at that composer's cursor (or over its selection), with a space where words would touch. Without `composer`, the one whose button was clicked last. Refused without a live grant for that composer, or once it has closed. |
| `composer.attach(file, { composer, name })` | Adds a picture (PNG, JPEG, GIF or WebP, up to 10 MB; a `Blob`, `File` or bytes) to that composer's next message. Same grant as `insert`. |
| `composer.setState(input, { state, label, title }, { composer })` | The button's look: `state` is `'idle'`, `'active'` (accent, breathing: listening, recording) or `'busy'` (quieter: working); `label` is a few words beside it (`'Listening 0:12'`); `title` its tooltip. With a `composer` you hold a grant for, in that composer only; every composer's otherwise. `'idle'` ends the button's grants. |

### Chats

Each call goes to the daemon, which checks the power in the install
record again. A plugin installed for one project reaches only that
project's chats. The answers are the daemon's own shapes.

| Call | Power | Does |
|---|---|---|
| `chats.list()` | `read_chats` | `{ chats: [{ id, title, harness, model, status, workspace, updated_at }] }`, every chat on this computer the plugin may reach. |
| `chats.read(id)` | `read_chats` | `{ text }`: the chat's messages, oldest first, as text. |
| `chats.start({ text, harness, workspace, model })` | `agent_tools` | Starts a chat on `harness` (required), in `workspace` (an absolute folder; a project's plugin starts in its project when none is named), with `text` as its first message. Answers `{ chat }`, the new chat's id. It runs with the harness's default permissions. |
| `chats.steer(id, text)` | `agent_tools` | Sends `text` to the chat: now, or after the running turn. Answers `{ queued: true }`. |
| `chats.stop(id)` | `agent_tools` | Stops the chat. Answers `{ stopped: true }`. |

### The network

`openremote.fetch(url, init)` (power `network`) works like `fetch`: it
takes `method` (GET, POST, PUT, PATCH, DELETE, HEAD or OPTIONS),
`headers`, `body` (text, bytes, a `Blob`, `URLSearchParams` or
`FormData`, not a stream), `signal`, and `timeoutMs` (120 s by default,
300 s at most), and answers a `Response`. The request is made by the
daemon, so the page's own limits don't apply, and nothing in it is
cached, redirected for you, or given cookies:

- Only `http` and `https`, and **only the public internet**. An address
  on this computer or its network (loopback, private, link-local and the
  other ranges [protocol.md](protocol.md) lists for WASM) is refused,
  **unless the user typed that address** into one of your plugin's own
  `string` settings: the request is then at or under an address the user
  saved. A setting's default never counts. So a server on this computer,
  a local transcription server for one, works once the user types its
  address (for example `http://127.0.0.1:8080/v1`) into your address
  setting.
- The connection goes to the very address that was checked; redirects
  come back as they are.
- The connection's own headers (`Host`, `Content-Length`, `Connection`,
  `Cookie`, `User-Agent` and the like, `Proxy-*`, `Sec-*`) are left out.
- A header value can name one of your secrets as `{{secret:key}}`:
  OpenRemote writes the secret in on the way out. A header whose secret
  is not set is left out, so a service that needs no key gets none. With
  `sendTo`, the secret goes only to the address the user saved in that
  setting: a request to any other address that names it is refused, and
  with no saved address the header is left out (a `required` secret's
  request is refused instead).

```js
const { values, secrets } = await openremote.settings.get()
const form = new FormData()
form.append('file', audio, 'speech.webm')
form.append('model', values.model)
const response = await openremote.fetch(`${values.base_url}/audio/transcriptions`, {
  method: 'POST',
  headers: secrets.api_key === 'set' ? { Authorization: 'Bearer {{secret:api_key}}' } : {},
  body: form,
})
```

### The microphone

`navigator.mediaDevices.getUserMedia({ audio: true })`, for a plugin given
the `microphone` power; refused otherwise. The system may ask the user
the first time. Let the microphone go (`track.stop()`) as soon as you are
done, so its indicator goes off.

### Settings, files, the app

| Call | Does |
|---|---|
| `settings.get()` | `{ values, secrets }`: every declared value (the default where none was saved), and for each secret `'set'` or `'unset'`. |
| `settings.onChange(handler)` | `handler({ values, secrets })` when the user saves your settings. |
| `files.read(path, { as })` | One of your own files: `as` is `'text'` (the default), `'arraybuffer'`, `'blob'` or `'dataurl'`. |
| `ui.toast(text)` | A short note at the corner of the app, under your plugin's name, for a few seconds. |
| `ui.openView(id)` | Opens one of your views (your first, without `id`). |
| `ui.openSettings()` | Opens Settings at your plugin's part. |

## How it works

For people working on OpenRemote itself:

- The frame boots from a small `blob:` page (`frameDocument` in
  `src/plugin-runtime.js`) carrying the frame's own policy (no address of
  its own, scripts only from the bridge and `blob:`) and the bridge,
  `public/plugin-bridge.js`, served by the console. The console's own
  policy (`src-tauri/tauri.conf.json`) allows frames from `blob:` only.
- The bridge and the console talk by `postMessage`: `{ openremote: 1,
  kind }`. The console (`src/plugins-host.jsx`) answers a message only
  from a frame it made: the message's source is that frame's window, its
  origin is the sandbox's opaque `"null"`, and its shape is the bridge's
  (`frameOf`). Every call is checked against the install record
  (`refusal` in `src/plugin-runtime.js`) and answered by
  `src/plugin-calls.js`, which also keeps the composer grants.
- The console reads a plugin's files through the daemon with its own
  token (`GET /plugins/{id}/files/{*path}`). Settings are `GET` and
  `POST /plugins/{id}/settings`, the network `POST /plugins/{id}/fetch`,
  and the chat calls `POST /plugins/{id}/call` with a closed list of
  methods, each checking the plugin's record again in the daemon
  (`daemon/crates/openremote-daemon/src/plugin_runtime/js.rs`). None of
  them answers the user's other devices.

# Writing an OpenRemote plugin

A plugin is a folder. It is Claude Code's plugin layout (skills, agents,
commands, hooks, MCP servers, with `.claude-plugin/plugin.json`) plus
OpenRemote's own part in `openremote.json` beside it, so one folder can be
a plugin to both. This guide covers the OpenRemote part: what a plugin
runs, what it adds to OpenRemote, and what it may do.

Examples in this repository:

- [`examples/plugins/dictation`](../../examples/plugins/dictation): a
  JavaScript plugin, a microphone button in every composer that writes
  what you say through an OpenAI-compatible transcription service.
- [`examples/plugins/text-tools`](../../examples/plugins/text-tools): a
  WASM plugin with three agent tools, written in Rust, one of them
  reaching public web pages. `build.sh` needs Rust's
  `wasm32-unknown-unknown` target.
- [`examples/plugins/toy-harness`](../../examples/plugins/toy-harness): a
  process plugin. One Rust program (`program/`, built by `build.sh` into
  `bin/toy`, `bin/toy.exe` on Windows) answers two agent tools,
  `word_count` and `greet`, and is also the driver of a toy harness,
  "Toy CLI", that echoes and asks before it shouts.
- [`examples/harness-plugin`](../../examples/harness-plugin): a copyable
  template for a harness plugin's driver.

The official catalog is the repository's own
`.claude-plugin/marketplace.json`.

The protocol a plugin's code speaks is in [protocol.md](protocol.md); the
JavaScript API is in [js-api.md](js-api.md); a harness driver's contract
is in [../harness-plugins.md](../harness-plugins.md).

## The folder

```
my-plugin/
  openremote.json          OpenRemote's part (this guide)
  .claude-plugin/
    plugin.json            Claude Code's part, when it has one
  skills/ agents/ commands/ hooks/ .mcp.json    Claude Code parts, if any
  ...                      the plugin's own files: its program, module, pages
```

A plugin without `openremote.json` is a Claude Code plugin; OpenRemote
installs and runs those too (its runtime is shown as "Claude plugin").

## openremote.json

```json
{
  "api": 1,
  "name": "toy-harness",
  "version": "0.2.0",
  "description": "A toy agent and two agent tools.",
  "runtime": "process",
  "entry": {
    "command": "bin/toy",
    "args": ["--tools"],
    "windows": {"command": "bin/toy.exe", "args": ["--tools"]}
  },
  "powers": [],
  "contributes": {
    "tools": [ ... ],
    "settings": [ ... ]
  }
}
```

| Field | Required | |
|---|---|---|
| `api` | yes | `1`. |
| `name` | yes, here or in `.claude-plugin/plugin.json` | Lowercase letters, digits and dashes, up to 64. Not a built-in harness's id. |
| `version` | no | Shown at install; updates compare it. |
| `description` | no | One line, shown at install. |
| `runtime` | yes | `process`, `wasm`, `js` or `content` (below). |
| `entry` | when it contributes something that runs | What it runs (below). |
| `powers` | no | What it may do (below). |
| `harness` | no | A harness the plugin adds: see [../harness-plugins.md](../harness-plugins.md). |
| `contributes` | no | What it adds to OpenRemote (below). |

Everything is checked when the plugin is staged for install. A mistake is
refused with a message that names it, and a `contributes` kind this
OpenRemote doesn't know is refused rather than ignored, so the user always
sees everything a plugin adds before approving it.

### Runtimes

- **`process`**: a program in the plugin's folder that OpenRemote starts
  and talks to over stdio ([protocol.md](protocol.md)). It can give agents
  tools and answer its own views. It **runs as you, not sandboxed**, and
  the install screen says so. It gets a cleared environment (only the
  variables protocol.md lists), so it can't count on whatever the daemon
  was started with.
- **`wasm`**: agent tools compiled to WebAssembly, run inside the daemon,
  sandboxed. It sees its own memory and only the host functions its powers
  grant (with `network`, HTTP requests to the public internet). Pick it
  for tools that compute or call web APIs.
- **`js`**: a plugin that lives in the console, in a sandboxed frame:
  composer buttons and views of its own ([js-api.md](js-api.md)).
- **`content`**: a Claude Code plugin with no OpenRemote code of its own
  (skills, agents, commands, hooks, MCP servers).

### entry

An entry is needed only when the plugin contributes something that runs:
a process plugin's tools, views or composer buttons, a wasm plugin's
tools. A harness plugin's driver is named by its `harness` block, not by
`entry`.

- A **process** plugin's entry is its program:
  - a path inside the plugin's folder, run as it is
    (`"entry": "bin/my-plugin"`), or
  - a command with arguments:
    ```json
    "entry": {
      "command": "bin/my-plugin",
      "args": ["--root", "${OPENREMOTE_PLUGIN_ROOT}"],
      "windows": {"command": "bin/my-plugin.exe", "args": []}
    }
    ```
    The command is a file inside the plugin's folder (written plainly or
    after `${OPENREMOTE_PLUGIN_ROOT}/`), checked after every link is
    followed, at install and before every start. `${OPENREMOTE_PLUGIN_ROOT}`
    in an argument stands for the folder. The optional `windows`, `macos`
    and `linux` entries replace the command on that system, and the one
    for the system you install on must be there at install.

    **A program on PATH is refused.** `node`, `python3` or `/bin/sh` is
    not the plugin's own file, and the cleared environment would not
    promise the same PATH anyway. Ship a compiled program (as the
    toy-harness example does), or a launcher inside the folder, for each
    system you support.
- A **wasm** plugin's entry is its module: `"entry": "tool.wasm"`. The
  install screen refuses a module that doesn't compile, doesn't export
  what OpenRemote calls, or imports more than its powers allow.
- A **js** plugin's entry is its page: a `.js` or `.html` file in its
  folder.

### powers

What a plugin may do, approved by the user at install, in these words:

| Power | The user sees |
|---|---|
| `run_programs` | Run programs on this computer |
| `read_chats` | Read chats and project files |
| `agent_tools` | Give agents tools, and start, steer and stop chats |
| `network` | Reach the internet |
| `microphone` | Use the microphone |

Some powers come with what a plugin is, and are shown like the rest
without being listed: a process plugin runs programs, a harness block runs
programs, a plugin with agent tools has `agent_tools`, and a Claude Code
plugin's hooks and MCP servers run programs.

**Powers come only from the install review.** What the user approved is
written into the plugin's install record, and every check reads the
record: editing `openremote.json` in the installed folder grants nothing,
nothing a request or a sync carries can add a power, and an update that
asks for a new power shows it at its own review. A plugin made by a
marketplace command that later asks for more is not swapped in; it waits
for an install from Plugins.

What the powers gate in OpenRemote's own surfaces:

- a WASM module may import `http` and `take` only with `network`, and
  they reach public addresses only;
- a process plugin's calls to OpenRemote, and a JavaScript plugin's chat
  calls: `chats/list` and `chats/read` need `read_chats`; `chats/start`,
  `chats/prompt` and `chats/stop` need `agent_tools`;
- a JavaScript plugin's `openremote.fetch` needs `network`, its
  microphone needs `microphone`;
- a harness driver gets OpenRemote's own MCP tools only with
  `agent_tools` (and the chat-reading ones also with `read_chats`).

A process plugin runs as you whatever it declares: declare honestly, the
user decides on what you say.

### contributes

`contributes` takes `tools`, `composer_inputs`, `views` and `settings`.
A harness is not a contribution: `contributes.harnesses` is refused, and
the plugin's `harness` block is the one place a harness is declared (the
review still lists it under what the plugin adds).

#### tools

Tools a chat's agent can call. Process and wasm plugins.

```json
"tools": [{
  "name": "word_count",
  "description": "Count the words, lines and characters in a piece of text.",
  "input_schema": {
    "type": "object",
    "properties": {"text": {"type": "string"}},
    "required": ["text"]
  },
  "read_only": true,
  "timeout_secs": 30
}]
```

- `name`: lowercase letters, digits and underscores, a letter first. The
  agent calls it `<plugin name>__<tool name>`, and its full MCP name
  (`mcp__openremote__<plugin>__<tool>`) must fit 64 characters.
- `description`: how the agent knows when to call it. Write it for the
  agent.
- `input_schema`: a JSON Schema object (MCP's `inputSchema` spelling
  reads too).
- `read_only`: the tool only reads. The agent sees it as MCP's
  read-only hint; it never skips the chat's own approval.
- `timeout_secs`: 1 to 600; 120 when left out.

Tools are listed from the install record, so listing them starts nothing;
a process plugin starts at its first call. Every chat on the machine gets
a Global or Machine plugin's tools; a chat in a project gets that
project's too. Two installed plugins with the same name can't both offer
tools to the same chats: the second install is refused.

#### composer_inputs

Buttons in the composer's bottom row, run by a JavaScript plugin's page.

```json
"composer_inputs": [{"id": "dictate", "label": "Dictate", "icon": "microphone"}]
```

- `id`: lowercase letters, digits and dashes.
- `label`: up to 40 characters.
- `icon`: one of the app's icon names, in letters only (`microphone`,
  `Microphone`); see [js-api.md](js-api.md) for the set.
- `entry`: the page, a path inside the plugin. A js plugin may leave it
  out: its own entry answers its buttons.

#### views

Pages of the plugin's own, run in a sandboxed frame from its files.

```json
"views": [{"id": "notes", "title": "Notes", "place": "sidebar", "icon": "notebook", "entry": "view.html"}]
```

- `place`: `sidebar`, `machines` or `settings`.
- `entry`: the page, a path inside the plugin.

Today the console shows a **JavaScript plugin's `sidebar` views** only: a
tile under Plugins in the sidebar that opens the view in a pane. Views
placed in `machines` or `settings`, and a process plugin's views (whose
page would reach its program through `POST /plugins/{id}/request`,
arriving as `ui/request`), are accepted and shown at review but have no
console surface yet.

#### settings

Values the user sets for the plugin, under Settings > Plugins.

```json
"settings": [
  {"key": "greeting", "type": "string", "label": "Greeting", "help": "Said before every reply.", "default": "Hello"},
  {"key": "token", "type": "secret", "label": "Token", "required": true},
  {"key": "verbose", "type": "boolean", "label": "Verbose logs", "default": false},
  {"key": "timeout", "type": "number", "label": "Timeout (seconds)", "default": 30, "min": 1, "max": 600},
  {"key": "region", "type": "enum", "label": "Region", "options": ["eu", "us"], "default": "eu"}
]
```

- `key`: lowercase letters, digits and underscores.
- `type`: `string`, `secret`, `boolean`, `number` or `enum`.
- `label` and `help`: what the user reads.
- `default`: must fit the type; a secret has none. A text or number
  field shows the default as its placeholder and holds only what the user
  typed.
- `options`: an enum's choices, as strings or `{"value", "label"}`.
- `min`, `max`: a number's bounds.
- `required`: the plugin doesn't run until it is set; a tool call says
  which setting is missing.
- `sendTo` (or `send_to`): a secret only, naming a `string` setting that
  holds an address. A JavaScript plugin's request that names the secret
  goes only to that address or under it ([js-api.md](js-api.md)).

The values are kept on each machine, in a file only the user can read.
Secrets never come back out through the API (only whether they are set)
and never leave the machine they were typed on: a Global plugin's
settings are set on each device. A process plugin gets the values in
`initialize` and `settings/changed`; a WASM plugin gets them in every
call's input; a JavaScript plugin reads the values, and each secret only
as set or unset.

## Install scopes

The user picks who a plugin is for when installing:

- **Every device** (`global`): this device installs it and tells the
  account. Another device of the account **installs nothing** when it
  hears: the plugin is listed under Plugins with a **Review** button, and
  that device's user goes through the same review and approval as any
  install, from a copy fetched from a device that has it and checked
  against the exact approved bytes. When one device removes it, the
  others **turn it off** and list it as removed on another device; each
  device's own user then removes it there (or turns it back on).
- **This machine** (`machine`): this machine only.
- **One project, outside its repo** (`project_local`): the files stay in
  OpenRemote's own folder; only chats in that project get it.
- **One project, in its repo** (`project`): the files go into the
  project's folder at `.openremote/plugins/<name>/`, with a note in
  `.openremote/plugins.json` of where it came from, so whoever clones the
  repo has them. Files are not approval: each machine reviews the plugin
  (Plugins lists a repo's new or changed plugins with a **Review**
  button) before it runs. What runs is a private snapshot of the files
  that machine reviewed, kept in OpenRemote's own folder, never the repo's
  live files; once the repo's copy changes, the plugin stops for that
  project until it is reviewed again.

A project's plugin reaches only its project's chats: its tools ride only
those chats, its chat calls refuse any other, and its composer buttons
and views show only there.

A plugin made by a marketplace's command installs for this machine only,
and a plugin with a harness can't be for one project.

## Trying a plugin

Install it from its folder: Plugins, install from a file or link,
Folder. The review shows what OpenRemote read. A process plugin's stderr
and its `log` lines appear on the daemon's stderr, prefixed with its id.

## Publishing in a marketplace

A marketplace is Claude Code's own catalog
(`.claude-plugin/marketplace.json`). OpenRemote reads what an entry says
about it for OpenRemote from the entry's `metadata.openremote`, so
Discover can show it before anyone installs:

```json
{
  "name": "dictation",
  "source": "./examples/plugins/dictation",
  "description": "Speak into the composer.",
  "metadata": {"openremote": {"runtime": "js", "powers": ["microphone", "network"]}}
}
```

The install still reads and checks the plugin itself, and the review
shows what the plugin says, not what the catalog says.

## Plugins written for a sibling layout

Some apps use a plugin layout built from Claude Code's: the same skills,
commands, agents, hooks and MCP servers, with its manifest in a folder of
its own (`.zcode-plugin/plugin.json`). OpenRemote installs such a plugin
as it is. At install it rewrites the **staged copy** (never the source)
into Claude Code's layout, and the review lists what changed under
**Adapted** and what it could not keep under **Left out**:

- Adapted: the manifest becomes Claude Code's (Claude Code's own manifest
  wins where both speak; authors, links, component paths and settings in
  the shapes Claude Code takes); the layout's path variables take Claude
  Code's names and the plugin's name and id are written in; a program
  hook becomes Claude Code's exec form with its time limit in seconds; an
  MCP server loses the fields Claude Code has no word for (its start
  folder, an off switch; `timeoutMs` becomes `timeout`) and a server with
  a `url` and no type is typed `http`; settings a plugin reads but never
  declares are declared, empty.
- Left out, each with its reason: an MCP server that signs in through an
  account at the plugin's publisher, a hook or server turned off in the
  plugin, one that needs an address only the publisher's app provides or
  a script this copy doesn't carry, and the plugin's own panels.
- A plugin with nothing left to run is refused with the reason, and
  Discover says so before install.

Catalogs in that layout work too: a `marketplace.json` at the top of the
folder (read when there is no `.claude-plugin/marketplace.json`), with a
top-level `pluginRoot` placed under every relative source. A download
source, `{"source": "url", "type": "zip", "url", "sha256", "path"}`, must
name a `sha256`, which is checked before anything is unpacked; its link
must be https (plain http only to this machine itself), and only the
folder `path` names inside the zip is unpacked. Names and descriptions
given per language read in English.

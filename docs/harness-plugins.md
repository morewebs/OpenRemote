# Harness plugins

A plugin can add a harness to OpenRemote without a change to the daemon or the
console. The plugin ships a driver: a small program that starts the harness's
own CLI and reports what it does in the daemon's own vocabulary. This page is
the contract. The Freebuff plugin is the worked example; the daemon's
`fixture-driver` crate is the smallest one.

A harness plugin is an ordinary OpenRemote plugin (see the Plugins view): a
folder installed from a folder, a git repository or an archive, with the same
powers disclosure. The `harness` block below is the only new part.

## The folder

```
my-harness/
  openremote.json
  bin/windows-x64/my-harness-driver.exe
  bin/linux-x64/my-harness-driver
  README.md
```

`openremote.json` is the plugin manifest, with a `harness` block:

```json
{
  "api": 1,
  "name": "my-harness",
  "version": "0.1.0",
  "description": "My harness as an OpenRemote harness",
  "runtime": "process",
  "powers": ["run_programs", "network", "agent_tools"],
  "harness": {
    "api": 1,
    "id": "my-harness",
    "name": "My Harness",
    "driver": {
      "windows": "bin/windows-x64/my-harness-driver.exe",
      "macos": "bin/macos-arm64/my-harness-driver",
      "linux": "bin/linux-x64/my-harness-driver",
      "default": "bin/my-harness-driver"
    },
    "args": [],
    "models": null,
    "default_model": null,
    "models_file": "models.json",
    "settings": "live",
    "permission_modes": [
      {"w": "ask", "l": "Ask first"},
      {"w": "auto", "l": "Run without asking"}
    ],
    "default_permission": "ask",
    "fast": false,
    "mcp": true,
    "commands": true,
    "one_prompt_at_a_time": true,
    "steers_mid_turn": false
  }
}
```

- `api` stays 1.
- `runtime` must describe a program (`"process"`), and the harness block adds
  `run_programs` to the powers the user approves.
- `harness.api` is this contract's version. Version 1 is the only one.
- `id` is lowercase letters, digits and dashes, up to 64. It must be unique on
  the machine and must not be one of `claude`, `codex`, `grok`, `pi`,
  `opencode`, `agy`.
- `name` is shown verbatim in the console.
- `driver` is a path inside the plugin folder, chosen by
  `std::env::consts::OS` (`windows`, `macos`, `linux`), with `default` as the
  fallback. `win32` and `darwin` are accepted as aliases. An absolute path or
  one containing `..` is refused.
- `models` is optional. When present it is an array in the shape the console
  already uses: `{"model": "...", "display_name": "...", "reasoning_efforts":
  ["..."], "is_default": true}`. `null` means the harness advertises nothing
  before a chat runs, and the console leaves the model slot reserved. Two more
  fields are optional on each entry: `detail`, a short second line in the
  harness's own words (a cost tier such as `3x · Recommended`), and `note`, the
  notice the harness prints under a name (`May use data for AI training`), which
  is shown as the row's hover title.
- `default_model` is optional and is shown before the first turn.
- `models_file` is optional. It names a file inside the plugin folder that the
  driver writes a model catalog into, in the shape `{"captured_at": <ms>,
  "default": "...", "models": [...]}`. The daemon reads it whenever it re-probes
  the harness, and a usable file wins over `models`: it is this machine's own
  reading, and `captured_at` is what the console's capture age shows. A file
  that is missing, malformed, empty or outside the plugin folder is not an
  error; the manifest's `models` list stands in. The Freebuff plugin is the
  example: its driver's `--catalog` mode writes `models.json`.
- `settings` is optional. The only word it takes is `"live"`, which says the
  driver can change the model on a running chat, between turns. The daemon then
  passes a model change straight to the driver and refuses it while a turn is
  working, because the driver cannot see the session's status. Without the key
  a change is stored on the chat and an idle driver is restarted onto it,
  resuming the same conversation (a working turn has to finish first). Any
  other word is refused at install, naming `live`, rather than being read as
  start-time.
- `permission_modes` is optional: the harness's own permission words, each a
  `{"w": "<word>", "l": "<label>"}` row (a bare string is accepted and is its
  own label). The console's permission picker lists them and the chosen word
  arrives as `spawn.permission_mode`, verbatim. Empty or absent means no
  permission picker. A word listed twice, or a row with an empty word or label,
  is refused at install.
- `default_permission` is optional: the word a fresh chat starts with. It must
  be one of `permission_modes`, or the plugin is refused at install.
- `fast` is optional (default `false`). `true` says the harness has a fast mode
  of its own and the driver reads `spawn.fast`; the console then offers the
  fast toggle.
- `mcp` is optional (default `false`). `true` says the driver hands
  `spawn.mcp_servers` and `spawn.allowed_tools` to its CLI. Without it both
  lists arrive empty. With it the driver gets the user's own MCP servers;
  OpenRemote's own tools also need powers the user approved at install (the
  top-level `powers`, shown on the install screen like `run_programs`):
  - `agent_tools`: OpenRemote's per-chat server (id `openremote`: the
    browser, the team and chat tools) and its pre-approved browser tools. Only
    then does the daemon tell the agent about the tools and offer team tools.
  - `agent_tools` and `read_chats`: also its pre-approved tools that read
    other chats (`mcp__openremote__agents_*`).

  A user server's command can carry a key, so a plugin should declare `mcp`
  only when its driver really hands the servers to its CLI.
- `commands` is optional (default `false`). `true` says the driver answers
  `--commands <folder>` (see "Commands" below), and the composer's `/` and `@`
  menus list what it answers. Without it no process starts for a menu, and the
  harness's own commands still run when typed.
- `one_prompt_at_a_time` is optional. Absent or `true`, a message sent while a
  turn runs is refused by the daemon before it reaches the driver. `false` says the driver takes a second `prompt` mid-turn on its
  own wire, which then opens a new turn.
- `steers_mid_turn` is optional (default `false`). `true` says a message sent
  while a turn runs lands inside that turn, and the harness closes the whole
  run with one `turn_completed`; the daemon files it under the running turn.
  It implies `one_prompt_at_a_time: false`, and saying both `true` is refused
  at install.

## Probe

At startup and after any plugin change, the daemon runs:

```
my-harness-driver --probe
```

It must print one JSON line and exit:

```json
{"available": true, "version": "1.2.3", "signed_in": true, "reason": null}
```

- `available` false hides the harness from pickers and shows `reason` in the
  Plugins list.
- `version` is the harness CLI's version, verbatim.
- `signed_in` is `true`, `false`, or `null` when the driver cannot tell. A
  guessed sign-in is worse than none.
- `reason` is one sentence for a person, and `null` when the harness is fine.

The probe must not start a chat or reach the network if it can avoid it. It is
bounded at five seconds.

## Commands

A driver whose manifest says `"commands": true` is asked for the composer's
menus when one opens:

```
my-harness-driver --commands <folder>
```

`<folder>` is the chat's workspace (or the home folder before a chat has one);
the driver's working directory stays the plugin folder. It must print one JSON
line and exit 0:

```json
{"commands": [{"name": "review", "description": "Review the diff", "argument_hint": "<path>", "aliases": ["r"]}],
 "agents": [{"name": "planner", "description": "Plans before it edits"}]}
```

- `commands` fill the `/` menu and `agents` the `@` menu. Only `name` is
  required; an entry without one is skipped. `{}` is an honest empty answer.
- A driver that cannot list them prints `{"reason": "<one sentence>"}`. The
  menu keeps what an earlier answer listed and asks again next time; so does a
  non-zero exit, output that is not JSON, or no answer within twenty seconds.
- The answer is cached per workspace. A `commands_listed` event (below) with a
  name the cached list lacks makes the next menu ask again.
- Like the probe, it must not start a chat. Its stderr is discarded.

## Session protocol

For each chat the daemon starts one driver process and speaks JSON lines over
its stdin and stdout. Every daemon message is a command; every driver message
is an event. The driver writes nothing but JSON to stdout. Write diagnostics
to stderr; the daemon forwards them to the chat.

Commands from OpenRemote:

| type | fields | meaning |
|------|--------|---------|
| `spawn` | `cwd`, `model`, `permission_mode`, `effort`, `resume`, `fast`, `mcp_servers`, `allowed_tools` | start the harness for this chat; `cwd` is the chat's workspace, `resume` is a `session_ref` from an earlier run |
| `prompt` | `text` | send a user prompt |
| `interrupt` | none | cancel the turn in progress |
| `answer` | `harness_ref`, `choice` | answer an approval the driver reported |
| `settings` | `model`, `effort`, `fast`, `permission_mode` | apply a live change, or refuse on stderr |
| `shutdown` | none | end the chat and the driver |

Events from the driver:

| type | fields |
|------|--------|
| `initialized` | `session_ref`, `model`, `permission_mode` |
| `model_reported` | `model` |
| `settings_refused` | `message` |
| `commands_listed` | `names` |
| `text` | `text` |
| `text_delta` | `text` |
| `reasoning`, `reasoning_delta` | `text` |
| `tool_started` | `tool_use_id`, `name`, `input` |
| `tool_result` | `tool_use_id`, `name`, `text`, `is_error` |
| `approval_requested` | `harness_ref`, `request`, `kind`, `options`, `tool_name`, `summary`, `interrupts_turn`, `session_grant` |
| `turn_completed` | `subtype`, `coarse`, `is_error`, `error_message` |
| `context_used` | `used`, `window` |
| `turn_cost` | `cost_usd` |
| `stderr` | `line` |
| `closed` | none |

Rules:

- `spawn` is answered by `initialized`. `session_ref` is the harness's own
  conversation id, which `resume` will be given on a later start. Without one,
  send an empty string.
- `model_reported` is the harness's own reading of the model a chat is on, after
  a start or a change; the console's chip follows it. `settings_refused` is one
  sentence saying why a change did not happen, and it lands as a note in the
  chat. Both are optional, and a driver that cannot read the model sends
  neither.
- `spawn.mcp_servers` is a list of `{"id", "command", "args"}` and
  `spawn.allowed_tools` names the tools that may run without asking, as
  `mcp__<server>__<tool>`. What they hold depends on the manifest's `mcp` and
  the approved powers (see `mcp` above); both are always lists, empty when
  nothing is handed. Hand them to your CLI's own MCP configuration.
- `commands_listed.names` are the command names the harness listed as the chat
  started. It is optional; it only tells the daemon its cached `/` menu is
  behind.
- `approval_requested.options` is a list of `{"id", "label"}`; an option
  without a label is shown as its id. `kind` is `approval` or `question`.
  `interrupts_turn` lists the option ids that end the turn instead of
  continuing it: the daemon does not mark the chat working again after one,
  and your own `turn_completed` settles it. `session_grant` lists the option
  ids that allow the tool for the rest of the chat: the daemon records
  `tool_name` on the chat. Both are optional; an option in neither continues
  the turn and grants nothing.
- `turn_completed.coarse` is `completed`, `interrupted` or `failed`, and
  `subtype` is the harness's own final word. A turn that ended in an error
  says `is_error: true` and puts the harness's own explanation in
  `error_message`.
- A second prompt while a turn is open is refused by the daemon before it
  reaches the driver, unless the manifest says `one_prompt_at_a_time: false`
  or `steers_mid_turn: true`. Only then does a driver receive a `prompt`
  mid-turn.
- On `interrupt`, send Escape (and Ctrl-C if your harness needs it), wait up to
  five seconds for your own end-of-turn signal, then emit
  `{"type":"turn_completed","subtype":"interrupted","coarse":"interrupted","is_error":false}`.
  That is the only boundary a driver may synthesize. Never synthesize a
  successful turn.
- On `shutdown`, end the harness and exit. The daemon kills the driver after
  three seconds.
- Unknown event types and unknown fields are ignored by this daemon, so a
  newer driver does not break an older one. Keep stdout JSON-only.

## What the console shows

Whatever the events say, and no more. A harness whose manifest declares no
models gets no model picker. A driver that never reports an approval gets no
approval cards. A driver that never reports context or cost gets no ring and
no cost row. That is deliberate: a dead control is worse than a missing one.

## Install and trust

A harness plugin runs a program as you, outside any sandbox. The install
screen says so because the plugin declares `run_programs`; read it before
approving. The daemon passes the driver no Cloud secrets, no daemon token and
no store: it inherits your environment, as every harness child does. Your MCP
servers and OpenRemote's own tools reach it only as far as its manifest's
`mcp` and the `agent_tools` / `read_chats` powers you approved allow.

While a chat on the plugin's harness is running, the driver executable lives
inside the plugin folder, so on Windows the plugin cannot be updated or
removed until that chat is stopped. OpenRemote refuses with the chat named
instead of an access-denied.

## Try a driver by hand

A driver is a program. Run it and type a command:

```sh
echo '{"type":"spawn","cwd":"."}' | ./my-harness-driver
echo '{"type":"prompt","text":"hello"}' | ./my-harness-driver
```

The second command prints the events a one-turn session produces. If you see
anything that is not JSON on stdout, fix that first.

## Reference

- `daemon/crates/fixture-driver/` in the OpenRemote repository: the smallest
  complete driver, with `--probe` and a scripted turn.
- `examples/harness-plugin/`: a copyable template.
- The Freebuff plugin: a real driver that owns a terminal, tails a transcript
  and maps it onto these events.

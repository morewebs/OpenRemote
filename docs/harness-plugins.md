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
  "powers": ["run_programs", "network"],
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
    "settings": "live"
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

## Session protocol

For each chat the daemon starts one driver process and speaks JSON lines over
its stdin and stdout. Every daemon message is a command; every driver message
is an event. The driver writes nothing but JSON to stdout. Write diagnostics
to stderr; the daemon forwards them to the chat.

Commands from OpenRemote:

| type | fields | meaning |
|------|--------|---------|
| `spawn` | `cwd`, `model`, `permission_mode`, `effort`, `resume`, `fast` | start the harness for this chat; `cwd` is the chat's workspace, `resume` is a `session_ref` from an earlier run |
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
| `text` | `text` |
| `text_delta` | `text` |
| `reasoning`, `reasoning_delta` | `text` |
| `tool_started` | `tool_use_id`, `name`, `input` |
| `tool_result` | `tool_use_id`, `name`, `text`, `is_error` |
| `approval_requested` | `harness_ref`, `request`, `kind`, `options`, `tool_name`, `summary` |
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
- `turn_completed.coarse` is `completed`, `interrupted` or `failed`, and
  `subtype` is the harness's own final word. A turn that ended in an error
  says `is_error: true` and puts the harness's own explanation in
  `error_message`.
- A second prompt while a turn is open is refused by the daemon before it
  reaches the driver: a plugin harness is one prompt at a time. Do not expect
  steering.
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
no store: it inherits your environment, as every harness child does.

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

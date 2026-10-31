# Harness plugins: shipping a harness as an OpenRemote plugin

Status: approach 1 approved (third-party harness plugins). Document pending
the owner's read.
Date: 2026-10-31.
Scope: the host-side seam in OpenRemote, and the Freebuff plugin that is its
first user.

## Intent and why

Freebuff is a free coding agent with a CLI, and it should be usable from
OpenRemote beside Claude Code, Codex, grok, pi, opencode and agy. The owner
does not want Freebuff support compiled into OpenRemote: it should arrive as
an installable plugin, so the core stays harness-neutral and the next harness
can ship the same way.

That is impossible today. Every harness is a Rust crate in the daemon
workspace plus wiring in the registry, supervisor, HTTP layer and console. A
plugin can carry Claude Code parts (skills, agents, commands, hooks, MCP
servers) or content, but no plugin can add a harness. So this work has two
halves:

1. a host-side seam in OpenRemote that lets an installed, enabled plugin
   provide a harness driver, and
2. the Freebuff plugin itself, written and released outside OpenRemote's tree.

## Goals

- A plugin folder can declare a harness; OpenRemote probes it, lists it, and
  runs chats through it with the same session vocabulary as the built-in six.
- Installing, enabling, disabling, removing or updating that plugin changes
  the harness list without a daemon restart.
- Freebuff support ships from its own repository and package. No Freebuff
  string, path or protocol shape appears in OpenRemote's source.
- The seam is generic enough that any third party can add a harness as a
  plugin, without another core change, and the protocol is documented for
  them.
- A plugin harness is honest about what it cannot do: no invented model list,
  no invented approvals, no guessed sign-in.
- The Freebuff plugin's driver owns every Freebuff-specific decision (TUI,
  PTY, transcript, resume).

## Non-goals

- Sandboxing plugin harnesses. They run programs as the user, like the
  built-in drivers.
- Changing the built-in six harnesses or their plugins.
- Letting a plugin replace a built-in harness id.
- A general JS or WASM plugin runtime. The seam is a process driver, not a
  new in-process extension ABI.
- Publishing the Freebuff plugin on a public marketplace in this first cut.

## Anti-goals

- A Freebuff-shaped branch in the daemon or the console.
- A plugin that names a driver outside its own folder, by an absolute path or
  by `..`.
- A harness row that looks ready when its driver or CLI is missing.
- Scraping the TUI's ANSI screen as the primary event source when the CLI
  writes a structured transcript.

## Constraints

- Windows first. The daemon runs console-less; every child carries
  `creation_flags()`.
- AGPL-3.0 for OpenRemote. No new dependency in the daemon: the driver owns
  the PTY.
- `unsafe_code = "deny"` and clippy `-D warnings` in the daemon workspace.
- The console's harness surface is data-driven from `GET /capabilities`
  (`harnesses`); harness ids are stable and display names come from the
  daemon.
- Freebuff CLI 0.2.35, measured on this machine, is TUI-only: no prompt
  argument, no `--print`, no `--json`, no ACP. It does take `-v`, `--cwd`,
  `--continue [conversation-id]`, `--trust-agents` and `login`. The TUI is
  OpenTUI + React and expects bracketed-paste input.
- Freebuff's chat store under
  `~/.config/manicode/projects/<project>/chats/<id>/` is internal and can
  change between CLI releases. It is a better source than screen scraping and
  it is never a contract.

## Approaches considered

### 1. Harness plugin as an external driver process (recommended)

OpenRemote grows one generic seam: a plugin's `openremote.json` can declare a
harness; the daemon spawns that plugin's driver for each chat, and the two
speak the driver vocabulary over JSON lines. Freebuff is the first plugin of
that kind, in its own repository.

- Core gains no harness-specific code.
- The seam is reusable: a later harness with any wire can be a plugin.
- Cost: a protocol to specify, version and test, plus the PTY and TUI work in
  the plugin.

### 2. A TUI recipe in core (rejected)

The plugin declares a launch command, a transcript path and a turn-end marker;
core grows a generic PTY harness that follows the recipe.

- Less to ship separately.
- The recipe vocabulary, and every Freebuff quirk (model picker, login, ads,
  resume), ends up in core. That is the thing the owner wants to avoid.

### 3. A terminal pane, not a harness (rejected)

The plugin adds a pane that runs Freebuff in a real terminal.

- Smallest and most honest.
- Freebuff stays outside chats: no history, tool cards, approvals,
  automations, Cloud or phone. That is not harness support.

This document specifies approach 1.

## Design

### 1. What a plugin declares

`openremote.json` keeps its existing fields (`api`, `name`, `version`,
`description`, `runtime`, `powers`) and gains an optional `harness` block:

```json
{
  "api": 1,
  "name": "freebuff",
  "version": "0.1.0",
  "description": "Freebuff CLI as an OpenRemote harness",
  "runtime": "process",
  "powers": ["run_programs", "network"],
  "harness": {
    "api": 1,
    "id": "freebuff",
    "name": "Freebuff",
    "driver": {
      "windows": "bin/windows-x64/freebuff-driver.exe",
      "macos": "bin/macos-arm64/freebuff-driver",
      "linux": "bin/linux-x64/freebuff-driver",
      "default": "bin/freebuff-driver"
    },
    "args": [],
    "models": null,
    "default_model": null
  }
}
```

Rules:

- `harness.api` must be 1. A plugin with another number is listed with the
  reason and its harness is not offered.
- `id` matches `[a-z0-9][a-z0-9-]{0,31}`, is unique, and is not one of the
  built-in ids or `stage`. A collision is listed with the reason and the
  later plugin is not offered.
- `name` is shown verbatim.
- `driver` is a path relative to the plugin folder, chosen by
  `std::env::consts::OS` with `default` as the fallback. The daemon resolves
  and canonicalizes it, and refuses anything outside the plugin folder.
- `args` are extra argv, with no shell expansion.
- `models` and `default_model` are optional static facts. `null` means this
  harness advertises nothing before a chat, and the console leaves the slot
  reserved.
- No new power is introduced. A harness plugin runs programs, which is
  `run_programs`, already disclosed at install, the same disclosure a hooks or
  MCP plugin gets.

### 2. Discovery and lifecycle

- `plugins::read_manifest` parses the block into a `HarnessManifest`.
- `HarnessRegistry::probe` gains a `plugins: &[PluginHarness]` argument.
  `App::new` passes the store's installed plugins;
  `Supervisor::swap_in_fresh_registry` passes the current list.
- A plugin harness is probed by running `<driver> --probe` with a five second
  timeout and the plugin folder as cwd. The driver prints one JSON line:

  ```json
  {"available": true, "version": "0.2.35", "signed_in": true}
  ```

  A missing driver or a failed probe yields `available: false`,
  `version: null`, `signed_in: null`, and the row says "not installed" or the
  probe's error. It never guesses a version or a sign-in.
- `install_staged`, `set_plugin_enabled`, `remove_plugin` and an update
  re-probe when the affected plugin declares a harness.
- A plugin harness appears in `GET /plugins` among the installed entries with
  `"kind": "harness"`, and in `GET /capabilities` `harnesses` like the built-in
  six. Disabling it hides it from pickers, exactly like disabling a built-in
  harness; its chats keep running.

### 3. Driver protocol v1

One driver process per chat. JSON lines over stdin and stdout; stderr carries
diagnostics. The daemon speaks; the driver reports events.

Daemon to driver:

| type | fields | meaning |
|------|--------|---------|
| `spawn` | `cwd`, `model`, `permission_mode`, `effort`, `resume`, `fast` | start the harness process for this chat |
| `prompt` | `text` | send a user prompt |
| `interrupt` | none | cancel the turn |
| `answer` | `harness_ref`, `choice` | answer an approval |
| `settings` | `model`, `effort`, `fast`, `permission_mode` | apply a live change, or refuse |
| `shutdown` | none | end the driver and its harness process |

Driver to daemon, one event per line:

| type | fields |
|------|--------|
| `initialized` | `session_ref`, `model`, `permission_mode` |
| `text`, `text_delta` | `text` |
| `reasoning`, `reasoning_delta` | `text` |
| `tool_started` | `tool_use_id`, `name`, `input` |
| `tool_result` | `tool_use_id`, `name`, `text`, `is_error` |
| `approval_requested` | `harness_ref`, `request`, `kind`, `options`, `tool_name`, `summary` |
| `turn_completed` | `subtype`, `coarse`, `is_error`, `error_message` |
| `context_used` | `used`, `window` |
| `turn_cost` | `cost_usd` |
| `stderr` | `line` |
| `closed` | none |

- Unknown event types are forwarded as stderr diagnostics, so a newer driver
  does not break an older daemon.
- A malformed line is a protocol error for that line only, not for the chat.
- `spawn` is answered by `initialized`. `settings` may be applied silently or
  refused with a `stderr` line.
- The driver writes nothing but JSON to stdout. Its own logs go to stderr.
- No request/reply ids in v1: every daemon message is a command, every driver
  message is an event. Models, commands and default model are static manifest
  facts, so no round trip is needed.

### 4. Daemon types and wiring

- `registry.rs`: `Backend::Plugin(PluginBackend)` and
  `SessionDriver::Plugin(PluginDriver)`, each method dispatching like the
  existing variants.
- `PluginBackend::spawn` starts the driver with `creation_flags()`, writes
  `spawn`, and returns a pump task.
- `PluginDriver` holds the child, its stdin, and a writer. It reports
  `per_prompt_process = false` and `steers_mid_turn = false`, so the
  supervisor's existing gates apply unchanged.
- `apply_settings` returns the harness-neutral refusal: a running Freebuff
  chat cannot switch model or effort; stop and resume. The console hides the
  controls when the manifest declares no models.
- `answer` refuses when the manifest declares no approvals.
- `models`, `default_model` and `commands` come from the manifest, or `None`.
- The event pump maps protocol events onto the existing `DriverEvent`
  vocabulary one for one.

### 5. Trust and safety

- The install approval screen already shows `run_programs` and "not
  sandboxed" for a `process` plugin. The harness block adds no capability.
- The driver path is confined to the plugin folder.
- The daemon never passes Cloud secrets, the daemon token or the store to the
  driver. The driver inherits the user's environment, as every harness child
  does today.
- A harness plugin is refused when its id collides with a built-in harness or
  another enabled plugin's harness id.
- A disabled plugin's harness is not offered, and its running chats are not
  killed.

### 6. Console surface

- `harnessName(id)` keeps its map and gains a fallback to the daemon's `name`
  where the full harness object is available. Today it falls back to the raw
  id.
- `HarnessMark` already returns null for an unknown id. Add a quiet monogram
  tile (the plugin's first letter) so a plugin harness is visible without a
  borrowed brand mark.
- The Plugins Installed tab shows the plugin as a plugin, with a "Harness"
  chip when it declares one. It is not listed in the built-in harnesses
  section.
- No new view and no new route.

### 7. The Freebuff plugin

A separate repository, working name `openremote-freebuff`:

```
openremote.json
bin/windows-x64/freebuff-driver.exe  built from driver/
driver/                           Rust crate: portable-pty, serde_json
README.md
LICENSE
```

Driver behavior:

- Resolve Freebuff: `%USERPROFILE%\.config\manicode\freebuff.exe`, then
  `$HOME/.config/manicode/freebuff`, then `freebuff` on PATH. The npm shim is
  not used: its launcher runs an update check that waits on a TTY.
- `--probe`: run `freebuff -v` (print and exit), report `available`, `version`,
  and `signed_in` from the presence of `credentials.json`.
- Session: spawn `freebuff --cwd <cwd> [--continue <id>]` in a 140x45 ConPTY
  with `TERM=xterm-256color`. Do not pass a prompt; the CLI takes none.
- Input: bracketed paste, `\x1b[200~` + prompt + `\x1b[201~` + `\r`. Wait for
  the first paint and 800 ms of quiet before the first prompt; afterwards the
  transcript's turn marker is the gate.
- Output: find the newest chat directory under the project's `chats/` after
  spawn, or use the resumed id. Poll `chat-messages.json` every 250 ms and
  emit:
  - `text` blocks as `AssistantText`, diffed by message id and length;
  - `tool` blocks as `ToolStarted` when first seen, and `ToolResult` when
    `output` appears;
  - `mode-divider` ignored.
- Turn boundary: tail `log.jsonl` for `"Main prompt finished"`, and report
  `turn_completed` with subtype `finished`. A line containing
  `Validation failed` or `fetch failed` reports an error turn with that line.
  Process exit reports `closed`.
- `initialized` fires when the chat directory is known: `session_ref` is the
  directory name, which is what `--continue` takes.
- Approvals: observation so far is that Freebuff LITE runs its tools without
  asking. The driver declares none and never invents a decision. If a real
  permission prompt appears, the driver reports it as an unsupported state
  with the screen text, rather than answering it.
- Context and cost: Freebuff reports neither to us, so the driver reports
  neither.
- Login: when credentials are missing, the probe says `signed_in: false`, and
  a chat start fails with "run `freebuff login` once in a terminal". A sign-in
  relay is a later feature.

What the console shows for Freebuff: the harness row, its signed-in state,
chats with assistant text and tool cards, a completed turn, and resume after
stop. It does not show a model picker, effort, fast mode, approvals, a context
ring or cost, because those are not known.

### 8. Third-party authoring

The point of the seam is that someone outside the project can add a harness.
That makes three things first-class deliverables, not documentation
afterthoughts:

- **A protocol reference** at `docs/harness-plugins.md`: the manifest schema,
  the message tables, the probe contract, the versioning rules, and a
  minimal worked driver. A plugin author should be able to write a driver
  against that page without reading the daemon.
- **A reference implementation.** The Freebuff plugin is a real one, but it
  is large (PTY, transcript, TUI). The daemon's test fixture is the small
  one: a driver of a few dozen lines that answers `--probe`, echoes a prompt
  as assistant text, reports one tool call and one completed turn. It lives
  in the test tree and is pointed at by the protocol page.
- **A template.** `examples/harness-plugin/` in the OpenRemote repository:
  `openremote.json`, a stub driver in Rust with the protocol loop, and a
  README that says what to fill in. It is not installed by the product; it
  is copied by an author.

Install and trust, stated plainly for authors:

- Third-party harness plugins install from a folder, a git URL or an archive,
  like any other plugin. The official marketplace's policy for process
  plugins is a separate decision and is out of scope for this cut.
- A harness plugin always declares `run_programs`, and the install screen
  says it runs programs outside any sandbox. That is the same disclosure a
  hooks or MCP plugin gets; there is no separate harness power.
- The daemon passes the driver no Cloud secrets, no daemon token and no
  store. It inherits the user's environment, as every harness child does.
- Harness ids are flat and global. A collision with a built-in id or another
  enabled plugin's id is refused and shown with the reason. No publisher
  prefix in v1; if collisions become real, a `publisher.id` form is a v2
  change to the same field.

### 9. Testing and acceptance

Core:

- unit tests for manifest parsing and id validation, including collisions and
  driver path escapes;
- registry probe against a fixture driver (a small test binary or a script)
  that answers `--probe`;
- an e2e chat through the fixture driver: spawn, prompt, text, tool, turn
  completed, shutdown;
- a test that enabling or disabling the plugin changes
  `capabilities.harnesses`;
- console tests for the unknown-name fallback and the monogram mark.

Freebuff plugin:

- `--probe` on this machine against 0.2.35;
- a live chat in a scratch folder with a prompt such as "read app.py and add
  type hints", verifying text and tool cards and a finished turn in
  OpenRemote;
- a stop and resume of that chat.

The live Freebuff run consumes part of the free daily allowance. It needs the
owner's go-ahead before it runs.

### 10. Versioning and compatibility

- `openremote.json` `api` stays 1. `harness.api` is the seam's own version.
- A driver whose `harness.api` the daemon does not know is listed, not
  offered, with "this plugin speaks harness API N; this OpenRemote speaks 1".
- Protocol additions are additive: unknown event types are diagnostics and
  unknown fields are ignored.
- Freebuff CLI upgrades are the plugin's problem: the driver pins a minimum
  version and refuses an older one with a clear message.

## Resolutions after the builder check (2026-10-31)

The review found four contracts that had to be pinned before an out-of-tree
driver could be written against them. They are decided here and override the
earlier wording where they differ.

1. **A live chat keeps the plugin folder in place.** The driver runs from the
   plugin folder, not a per-chat copy. On Windows an executing image locks its
   folder, so updating or removing a plugin whose harness has a live chat is
   refused with an error that names the chat: "stop the chat first". Enabling
   and disabling never touch files and stay instant. No copy-and-GC contract
   is invented in v1.
2. **A plugin harness takes one prompt at a time.** It advertises
   `per_prompt_process = true`, so the supervisor refuses a second prompt
   while a turn is open with its existing message instead of writing a prompt
   the TUI would swallow. On `interrupt` the driver may synthesize the turn's
   boundary after a bounded cancel: send Escape (and Ctrl-C if needed), wait
   up to five seconds for the harness's own marker, then emit
   `turn_completed` with `subtype: "interrupted"` and
   `coarse: "interrupted"`, `is_error: false`. That is the only synthesized
   boundary a driver may send; a successful turn is never invented.
3. **OS keys are `windows`, `macos` and `linux`** (the words
   `std::env::consts::OS` returns), plus `default`. The Node words `win32` and
   `darwin` are accepted as aliases so a hand-written manifest still works,
   but the documented spelling is the Rust one. One driver per OS; a path may
   encode the architecture (`bin/windows-x64/...`).
4. **The probe gains a reason and the row can show it.** `--probe` answers
   `{"available": bool, "version": string|null, "signed_in": bool|null,
   "reason": string|null}`. `Harness` gains an optional `problem` string; the
   console shows it in the row's detail slot when the harness is not usable.
   `models` is a typed array of the existing `ModelDescriptor` shape
   (`model`, `display_name`, `resolved_model`, `reasoning_efforts`,
   `is_default`); `commands` is not in v1 and is an additive field later, not
   a `harness.api` bump.
5. **Plugin harnesses are machine harnesses.** They appear in
   `GET /capabilities`, in a machine's view and in peer routing exactly like
   the built-in six, so a paired phone or another device of the account may
   start one on this computer. That follows the machine contract: a peer can
   already drive any installed harness. The trust-model note gains a line
   saying so, and the acceptance test covers a peer-created session.



## Risks and open questions

- **TUI coupling.** Freebuff can change its TUI or transcript without notice.
  Mitigation: transcript first, a version guard, clear failures, and an
  acceptance run per release. If the transcript moves, the plugin needs an
  update, not OpenRemote.
- **PTY on Windows.** ConPTY through `portable-pty` must be verified in a
  spike before the rest of the plugin is built.
- **Driver distribution.** The first cut ships a Windows x64 binary in the
  plugin repository, built locally. A release workflow and per-platform
  assets come later.
- **Transcript write cadence.** Whether `chat-messages.json` updates during a
  turn or only at turn end must be measured live. If it is turn-end only,
  text arrives in one burst and `text_delta` is dropped; the chat is still
  correct.
- **Approvals.** If Freebuff starts asking for permission, the driver must
  either surface the ask or declare it unsupported. It must not answer blindly.
- **Marketplace.** The official marketplace may not carry a process harness
  plugin in this cut; install from git or a folder.

## Builder calls

These are decided here, with the reason, rather than left open:

- The seam is a process protocol, not an in-process ABI.
- The protocol reuses the daemon's existing driver vocabulary rather than
  inventing a second one.
- Freebuff is Windows-first; other platforms come with the release workflow.
- The Freebuff driver owns the PTY; OpenRemote gains no PTY dependency.
- The plugin declares no models, approvals, effort or fast mode until the
  driver can report them truthfully.
- Unknown harness ids get a monogram tile, not a borrowed brand mark.

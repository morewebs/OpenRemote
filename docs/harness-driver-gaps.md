# Harness driver gaps

**What the six drivers expose, what their harnesses offer, and where the two differ.**

**Date:** 2026-11-02
**Build:** OpenRemote 0.4.8, console commit `1f6dc13`, daemon 0.4.0
**Scope:** `daemon/crates/openremote-{claude,codex,grok,pi,opencode,agy}`, the
`openremote-harness` vocabulary, the registry that advertises each harness, and the
plugin driver seam.
**Status:** internal. No product code was changed.

---

## Status on 2026-11-13 (0.5.0 plus the working tree)

Re-checked against the code, item by item. The rest of this page is the
11-02 reading and is kept as it was.

| Item | Now |
|---|---|
| G1, G2 | Fixed: Grok passes `--reasoning-effort` and `--include-partial-messages` (e998295) |
| G3 | Partly fixed: Claude and Codex read `allowed_tools`, and `honors` says per App whether MCP is read (28664c7). Grok, Pi, OpenCode and Antigravity still ignore `mcp_servers` |
| G4 | Fixed: Antigravity's three mode words are advertised (e998295) |
| G5, G23 | Open by design: Codex and Grok endpoint records can be saved, Settings says they can't run yet, and only Claude executes one. Finishing them needs each CLI's provider wire checked against a real endpoint |
| G6 | Fixed: a plugin harness answers its own approval (f1eca72) |
| G7 | Fixed: a new Pi chat keeps its transcript in OpenRemote's data folder, one folder per workspace (`harness-state/pi/sessions/<name>-<hash>`). A chat whose transcript is already in the workspace's `.pi-sessions` keeps resuming from there; nothing is moved or deleted |
| G8 | Permission word fixed (eae6816, 0008da3). The v1 reply paths are still unverified, since no v1 server has been run |
| G9 | Claude, Codex and now Pi take pictures (Pi as `images` on its rpc prompt) |
| G10 | Pi now steers: every prompt carries `streamingBehavior: "steer"`, which Pi reads only while a run is going. OpenCode still does not |
| G11 | Grok and Antigravity report context (6b7f2f3); Grok cost. Pi now reports context and cost from each assistant message's own `usage` |
| G12 | Grok's `slash_commands` now reach the composer: the names a chat lists on init stand in for the list Grok cannot give before a chat runs |
| G13, G14, G15 | Open |
| G16 | Pi uses `steer` (through `prompt`), `images` and the run events; `get_session_stats`, `compact` and the session commands are still unused |
| G17 | Open: Antigravity still shows no tool activity |
| G18 | Fixed: Grok reports each `tool_use` as a started tool with its own id and name, and the result under the same name |
| G19 | Open |
| G20 | Fixed: `Harness.honors` (28664c7) |
| G21 | Fixed: the references to the missing audit are gone from the code |
| G22 | Fixed: Grok's crate doc describes `--prompt-file` and the documented wire; the three shutdown comments say what `kill_and_reap` does |

Found while re-checking, not in the list above, and fixed:

- **A Pi turn ended at its first assistant message.** The reader closed the
  turn on any assistant `message_end`, including one that stopped for a tool
  call, so every Pi turn that used a tool was marked done while Pi kept
  working, and the tool cards and the answer landed after it. Pi's own
  boundary is `agent_settled`; the turn now ends there, once.
- **Pi tool results were always empty**, because the result is a body of
  `content` blocks and was read as a string, and **never marked as
  failed**, because `isError` was not read.
- **Grok compaction** (`compact_boundary`) was dropped; it is now the same
  note Claude's is. **Pi compaction** (`compaction_end` with a result) too.

---

## 0. The short version

The driver layer is in better shape than the product surface it feeds. Every
driver invents nothing: prompts never ride argv, every turn boundary is the
harness's own frame, and no driver synthesizes a success. The gaps are of a
different kind - facts the daemon already computes and the drivers throw away.

- **Three of eleven spawn options reach every driver** (`cwd`, `model`,
  `resume`). `effort`, `fast`, `endpoint`, `mcp_servers`, `plugin_dirs` and
  `allowed_tools` reach some or none.
- **MCP servers are handed to all six drivers and read by two.** The daemon's
  own read-a-chat server rides every spawn (`supervisor/composer.rs:123-142`),
  so `@`-mentioning a chat only works on Claude Code and Codex. On Grok, Pi,
  OpenCode and Antigravity the mention is inert text.
- **`allowed_tools` is computed and read by no driver, including Claude.** The
  built-in browser's read-only tools therefore ask for approval on every call.
- **Grok's effort control is live in the console and dropped in the driver**
  (`build_args` never emits `--reasoning-effort`), and Grok never streams text
  because `--include-partial-messages` is never passed while delta frames are
  handled.
- **Antigravity's `--mode` is implemented and never advertised**, so the one
  App with a working execution-mode wire has no mode picker.
- **The plugin seam cannot answer an approval.** `PluginDriver::answer` returns
  an error unconditionally, so the `approval_requested` event the contract
  documents produces a card that cannot be clicked.

Coverage of the seventeen driver events, per App, ranges from **16 (Claude) to
6 (Antigravity)**.

---

## 1. Method

Three tests decide whether something here is a gap.

1. **Set and dropped.** The console or an API call sets a fact on a chat, the
   daemon resolves it into `SpawnOptions`/`SessionSettings`, and the driver
   ignores it without an error. The user sees a control that does nothing.
2. **Offered and unused.** The installed CLI documents a surface (a flag, a
   subcommand, a protocol call) and the driver has no code for it.
3. **Not expressible.** The harness-neutral contract has no field or event for
   a capability, so no driver could expose it even if it wanted to.

Sources, in this order:

- The code at `1f6dc13`, read in full for each driver crate. Every claim below
  names a file and line.
- Each harness CLI's own `--help` and subcommand help, run on this machine:
  codex 0.165.0, grok 1.0.70, pi 1.4.0, opencode 1.19.3, agy 1.4.10. Pi's RPC
  surface comes from the `docs/rpc-commands.md` shipped in its installed
  package.
- **Claude Code's CLI is not installed on this machine.** The Claude column is
  read from the driver and from `docs/harness-plugins.md`, not from the CLI's
  help. Treat Claude's "harness offers" claims as unconfirmed here; its driver
  behavior is not in doubt.

Not verified on this page: no live chat was run. Anything that depends on a
signed-in model (whether Grok emits deltas when asked, whether Claude's cost
field rides this transport) is marked as such.

---

## 2. The layer

A **driver** is one crate that starts one harness CLI and turns its frames into
`DriverEvent`s (`daemon/crates/openremote-harness/src/events.rs:36-126`). The
daemon resolves every chat into `SpawnOptions` once per spawn
(`openremote-harness/src/lib.rs:120-157`) and `SessionSettings` for live
changes (`:186-199`), then dispatches through `SessionDriver`
(`openremote-daemon/src/registry.rs:45-200`).

The console renders only what the events report. Its own docs say so:
"Whatever the events say, and no more" (`docs/harness-plugins.md:149-154`). A
driver that says nothing produces no control, and that is the intended
behavior. It is also why every silent drop below is invisible.

| App | Transport | Wire |
|---|---|---|
| Claude Code | one process per chat | `stream-json` in and out, control protocol on stdio |
| Codex | one process per chat | `codex app-server`, JSON-lines RPC |
| Grok | one process per prompt | `streaming-messages-json` (Anthropic wire) |
| Pi | one process per chat | `pi --mode rpc`, JSON-lines RPC |
| OpenCode | one `opencode serve` per chat | HTTP plus SSE |
| Antigravity | one process per prompt | `stream-json` in and out (written one line, then closed) |

---

## 3. Capability matrix

`Y` = exposed and working. `-` = no code, or the harness has no such surface.
`~` = degraded or conditional (see the note). `Q` = questions only, no
approvals. `fixed` = present but hardcoded, not user-chosen.

| Capability | Claude | Codex | Grok | Pi | OpenCode | Antigravity |
|---|---|---|---|---|---|---|
| Persistent process | Y | Y | - | Y | Y | - |
| Streaming text | Y | Y | ~ (1) | Y | Y | Y |
| Reasoning stream | Y | ~ (2) | ~ (1) | - | ~ (3) | - |
| Thinking-token count | Y | - | - | - | - | - |
| Tool started card | Y | Y | - | Y | Y | - |
| Tool result card | Y | Y | ~ (4) | ~ (5) | Y | - |
| Approval cards | Y | Y | - | Q | Y | - |
| Session-wide grant | - | Y | - | - | Y | - |
| Steer a running turn | Y | Y | - | - | - | - |
| Context used | Y | Y | - | - | - | - |
| Context window | - | Y | - | - | - | - |
| Cost in USD | ~ (6) | - | Y | - | - | - |
| Compaction marker | Y | Y | - | - | - | - |
| Commands + agents catalog | Y | Y | - | Y | Y | - |
| Commands refreshed live | Y | - | - | - | - | - |
| Models catalog | Y | Y | Y | - (7) | Y | Y |
| Effort honored | Y | Y | - (8) | Y | - | - (9) |
| Fast mode | Y | Y | - | - | - | - |
| Permission words offered | 4 | 4 | 6 | 0 | 0 | 0 (10) |
| Custom endpoint | Y | - | - | - | - | - |
| MCP servers injected | Y | Y | - | - | - | - |
| Plugin folders loaded | Y | - | - | - | - | - |
| Image or file attachment | - | - | - | - | - | - |
| Extra workspace roots | - | - | - | - | - | - |
| Sandbox chosen by user | - | fixed (11) | - | - | - | - |
| Sign-in fact reported | Y | Y | Y | - | Y | - |

Notes:

1. Grok handles `text_delta` and `thinking_delta`, but `build_args` never passes
   `--include-partial-messages`, the flag Grok documents as the switch that
   emits them. Without it the reply arrives as whole messages
   (`openremote-grok/src/driver.rs:218-242`, `anthropic_wire.rs:30-35`).
2. Codex reports reasoning as a whole item on `item/started`
   (`openremote-codex/src/driver.rs:939-945`), never as a delta.
3. OpenCode forwards `session.reasoning.delta` (`openremote-opencode/src/driver.rs:468-472`)
   and has no matching "ended" mapping, so a reasoning block is deltas only.
4. Grok reports tool results with an empty `tool_use_id` and the hardcoded name
   `commandExecution` (`openremote-grok/src/driver.rs:393-403`), and never
   reports the matching call.
5. Pi reports `is_error: false` for every tool result
   (`openremote-pi/src/driver.rs:647`).
6. Claude reads `total_cost_usd` when the frame carries it
   (`openremote-claude/src/driver.rs:910-918`). There is no `-p` on this
   transport and no real-CLI test, so whether it appears is unverified.
7. Pi's catalog is `Ok(None)` by decision: `pi --list-models` answers "No models
   available" until a provider is signed in (`registry.rs:312-315`). The
   reserved slot means no model picker and, because effort tiers hang off
   models, no effort picker either.
8. Grok's `--reasoning-effort` exists and is never passed (see G1).
9. Antigravity's effort rides inside the model id (`gemini-3.8-flash-high`), and
   `agy models` reports it that way, so the empty effort list is honest
   (`openremote-agy/src/driver.rs:469-475`). `--effort` itself is unused.
10. Antigravity's driver does pass `--mode` (`openremote-agy/src/driver.rs:202-205`)
    with the CLI's three words, but `permission_modes("agy")` is empty
    (`registry.rs:822-828`), so no picker renders (see G4).
11. Codex's sandbox is the constant `workspace-write`
    (`openremote-codex/src/driver.rs:30,743`). The CLI offers `read-only`,
    `workspace-write` and `danger-full-access`.

---

## 4. Set and dropped

### G1. Grok's effort control does nothing

The console offers effort tiers from Grok's own catalog, the daemon resolves
the pick into `SpawnOptions.effort` on every spawn and resume
(`supervisor.rs:1440`, `:2039`), and `build_args` emits model, permission mode,
resume and the prompt file - not effort (`openremote-grok/src/driver.rs:218-242`).
A user who picks `high` gets Grok's config default.

This is the only harness whose effort is read (for display,
`openremote-grok/src/config.rs:118-151`) and then dropped on the way to the CLI.
Every other driver that has an effort wire passes it.

**Cheapest fix:** one `push_word(&mut args, "--reasoning-effort", effort)` in
`build_args`, plus a fixture check. Half a day including the e2e.

### G2. Grok never streams

The driver maps `stream_event` text and thinking deltas
(`openremote-grok/src/driver.rs:405-410`), and the shared parser documents that
those frames arrive "only with `--include-partial-messages`"
(`openremote-harness/src/anthropic_wire.rs:30-35`). The flag is never passed.
Grok answers arrive as whole blocks, and the delta branches are dead code.

Same fix shape as G1, and the e2e for it already exists in the Anthropic wire
tests.

### G3. MCP servers, plugin folders and allowed tools reach six drivers and are read by two

`SpawnOptions` carries `mcp_servers`, `plugin_dirs` and `allowed_tools`. The
supervisor fills all three on every spawn (`supervisor.rs:1436-1447`,
`:2035-2046`):

- `mcp_servers` includes every enabled MCP server on the machine **and**
  OpenRemote's own read-a-chat server, built per chat
  (`supervisor/composer.rs:123-142`).
- `allowed_tools` is the built-in browser's read-only tool names
  (`supervisor/composer.rs:144-153`).

What each driver does with them:

| | claude | codex | grok | pi | opencode | agy |
|---|---|---|---|---|---|---|
| `mcp_servers` | `--mcp-config` | `mcp_servers.<id>.*` | ignored | ignored | ignored | ignored |
| `plugin_dirs` | `--plugin-dir` | ignored | ignored | ignored | ignored | ignored |
| `allowed_tools` | ignored | ignored | ignored | ignored | ignored | ignored |

Two consequences are user-visible today:

- **`@chat` mentions only work on two Apps.** A mention is recorded on the
  session and nothing else (`supervisor/composer.rs:288-319`); the only reader is
  `read_mentioned` (`:351-359`), which backs the `read_chat` tool served by the
  `openremote` MCP server (`openremote-daemon/src/mcp_chat.rs:114-118`). On
  Grok, Pi, OpenCode and Antigravity the model is handed text pointing at a tool
  it was never given, and the user gets nothing, with no message saying why.
- **The browser's read tools ask for approval on Claude, the only App that
  loads them.** `--allowedTools` is never passed
  (`openremote-claude/src/driver.rs:730-769`), so every read, hover and scroll
  is a decision card.

None of this is stated anywhere a user can see. `docs/harness-plugins.md`
describes plugins as riding `SpawnOptions.mcp_servers` and then notes "a
harness with no wire for them simply doesn't run plugins" (`:144-152`), which is
the honest sentence the console never shows.

**Cheapest fix:** the drivers that cannot honor a field should refuse it the way
they refuse endpoints (below), and the console should dim the MCP row per App.
The bigger fix is G9's capability advertisement.

### G4. Antigravity's execution mode is wired and unseen

Antigravity's CLI takes `--mode accept-edits|auto|plan`. The driver passes it
(`openremote-agy/src/driver.rs:202-205`). The registry advertises no permission
words for `agy` (`registry.rs:644-647`, `:822-828`) and no default
(`:683-690`), so the console renders no picker and every Antigravity chat runs
in whatever mode the CLI defaults to.

It is worse than a missing control: `permission_mode` still flows from the API
through the store into the next spawn with only an empty-string filter
(`http.rs:2770`, `supervisor.rs:1692-1698`), shape-checked alone
(`openremote-agy/src/driver.rs:235-245`). A caller can set `auto` on an
Antigravity chat and it takes effect silently, with no control in the UI that
shows it.

**Fix:** add the three words to the registry. The driver is already done, and
this also removes the only path by which an invisible permission word can be
set.

### G5. Two of the three harnesses that accept a custom endpoint cannot use one

`validate_endpoint_harness` accepts `claude`, `codex` and `grok`
(`openremote-core/src/model.rs:218-225`), the endpoints API enforces that list
(`http.rs:365-377`), and Settings will therefore save a Codex or Grok endpoint
record. Execution is gated to Claude in two places:
`custom_endpoint_supported` is `matches!(id, "claude")` (`registry.rs:637-639`)
and `Backend::spawn` rejects any other harness (`:209-216`). The grok driver
refuses again in its own spawn (`openremote-grok/src/driver.rs:60-64`).

The stored record is not garbage: a chat can be pointed at it and the failure
arrives at spawn. But the console's own copy already says "only claude executes
custom endpoints today" (`src/EndpointManager.jsx:272`), so the gap is between
the validator and the executor, not between the UI and the truth.

**Fix:** one list, one constant. Either narrow the validator to `claude`, or
finish the Codex and Grok bindings the registry comment promises.

### G6. A plugin harness can show an approval and never answer it

`docs/harness-plugins.md` documents `approval_requested` as a driver event and
`answer` as a daemon command (`:99-125`). The seam cannot deliver the second:
`PluginDriver::answer` returns `DriverError::Harness("this harness asks for no
approvals")` unconditionally (`openremote-daemon/src/plugin_driver.rs:197-208`),
and `SessionDriver::answer` calls it and maps the success path to a default
outcome (`registry.rs:157-160`).

So a third-party driver that emits an approval gets a card in the console whose
buttons always fail. Nothing ships that does this yet - the Freebuff driver
documents the refusal as a deliberate protocol answer
(`Projects/OpenRemote-freebuff/PROTOCOL.md:172-176`) - so the bug is latent,
which is the best time to fix it.

**Fix:** write `{"type":"answer","harness_ref":"<ref>","choice":"<choice>"}` down
the pipe in `PluginDriver::answer` and let the driver's own `AnswerOutcome` come
back as an event, or delete `approval_requested` from the contract. A documented
event that cannot complete is worse than an event that does not exist.

### G7. Pi writes session state into the user's workspace

`build_args` passes `--session-dir .pi-sessions`, relative
(`openremote-pi/src/driver.rs:414-420`), and the child's cwd is the chat's
workspace. Every Pi chat therefore creates a `.pi-sessions` folder inside the
user's repository. This repository had to add the name to its own
`.gitignore:24` to stay clean.

Two effects: a user's `git status` picks up an untracked folder, and a chat
whose workspace moves cannot reach its transcript by id, because the id lookup
is rooted at the current cwd.

**Fix:** an absolute path under the data dir, or `OPENREMOTE_*` config.

### G8. OpenCode's permission word is dropped, and its v1 reply routes are wrong

Two defects in one crate:

- `SpawnOptions.permission_mode` is never read, and the driver's `Initialized`
  hardcodes `permission_mode: None` (`openremote-opencode/src/driver.rs:330-336`).
  OpenCode has no permission words in the registry, so no UI sets it, and a
  peer-set word is silently ignored rather than refused.
- `prompt` and command sends branch on the wire version (`v1` vs `v2`,
  `:813-824`), but the reply, question, form and interrupt calls hardcode
  `/api/...` paths (`:151-221`). A v1 server that accepts a prompt would answer
  404 to an approval or an interrupt.

**Fix:** branch the reply paths the way the prompt path already does, and refuse
a permission word at spawn instead of dropping it.

---

## 5. Offered and unused

### G9. No attachment path exists, for any App

`SpawnOptions` has no attachment field and `send_prompt` takes `&str` on every
driver. The console has `src/attachments.js`, a complete per-composer store for
images and picked elements, **with no importer anywhere in `src/`** - the same
fact the gap map records as E1.

The harnesses are ready for it. Two document image input outright: Codex
`-i/--image <FILE>...`, and Pi's RPC, where `steer` and `follow_up` take
`images: [{data, mimeType}]`. Grok (`--prompt-json`) and Antigravity
(`--input-format stream-json`) both take a prompt as JSON content blocks in the
Anthropic message shape, where an image block is a standard content type; neither
driver builds one. Claude's stream-json is the same message wire. OpenCode is
the one App where pictures would need checking against its HTTP API.

This is a contract gap, not a driver gap, and it is the one that most changes
what the composer can do.

### G10. Steering exists on two Apps

`SessionDriver::steers_mid_turn` returns true for Claude and Codex only
(`registry.rs:190-199`). Pi has `steer`, `follow_up` and `clear_queue` in its
RPC and the driver sends only `prompt` and `abort`
(`openremote-pi/src/driver.rs:249-273`). OpenCode is held out "until their own
mid-turn behavior is verified on a real install" (`registry.rs:194-199`).

On Grok and Antigravity a message typed during a turn is refused with a
Conflict (`supervisor.rs:1522-1533`), which is correct for a per-prompt process
and still a worse experience than the four persistent Apps.

### G11. Usage facts land on two or three Apps

| Fact | Apps that report it |
|---|---|
| Context used | Claude, Codex |
| Context window | Codex only |
| Cost | Claude (unverified on this transport), Grok |
| Compaction marker | Claude, Codex |
| Thinking tokens | Claude |

Grok parses `usage` on the result frame and discards it
(`openremote-grok/src/driver.rs:411-417`). Antigravity's result frame carries
`usage` and the driver reads only `response`, `status` and `error`
(`openremote-agy/src/driver.rs:302-333`). Pi has `get_session_stats` in its RPC
and never calls it. OpenCode reports tokens and cost through its own `stats`
command, which this driver never reads.

So the context ring in the chat header is a Claude and Codex feature, and the
cost row is a Grok feature.

### G12. Two Apps cannot show a command list

Claude, Codex, Pi and OpenCode list commands and agents; Grok and Antigravity
return `Ok(None)` (`registry.rs:287`). Grok's own `system/init` frame carries
`slash_commands`, the parser already extracts it
(`anthropic_wire.rs:147-156`), and the driver drops it with `..`
(`openremote-grok/src/driver.rs:357-378`). Antigravity expands slash commands and
skills in print mode, by its own `--disable-slash-commands` flag, and has an
`agents` subcommand; none of it reaches the composer.

The comment says the lists wait "until their lists are verified on signed-in
installs" (`registry.rs:258-263`). Grok's list arrives free on a frame the
driver already parses, so at minimum the running chat could refresh it
(`CommandsListed`).

### G13. No sandbox or extra-root control, on any App

Codex has three sandbox modes, Grok `--sandbox <PROFILE>`, Antigravity
`--sandbox`, and all three have an `--add-dir`. None of it is in the contract:
`SpawnOptions` has one `cwd` and no sandbox field. Codex hardcodes
`workspace-write` (`openremote-codex/src/driver.rs:30,743`), so the one App
whose sandbox is real cannot change it.

This is the driver-layer half of the gap map's A1/A2. A worktree or a container
is the product answer; "the App's own sandbox mode, chosen per chat" is the
cheap answer, and it needs one contract field.

### G14. ACP is not used anywhere

`opencode acp` starts an Agent Client Protocol server, and the market
comparators (Cloud Room, T3 Code) treat ACP as the way to add a harness cheaply.
OpenRemote's answer is the plugin driver contract, which is bespoke JSON lines
over stdio. That is a defensible choice - it gives the console approvals, tool
cards and cost that ACP may not carry - but it means the sixth harness costs a
crate, not a manifest. The gap map already lists this as E9; the driver-layer
point is that no crate speaks ACP and nothing in the tree evaluates it.

### G15. Per-prompt Apps serialize, and Antigravity chooses to

Grok and Antigravity are one process per prompt, so a second prompt during a
turn is a Conflict, and an interrupt is `kill_tree`, not a protocol message.
For Grok that is forced by print mode. For Antigravity it is a choice: the
driver already runs `--input-format stream-json`, which the CLI documents as
"one NDJSON message per line, runs a turn for each", and then closes stdin after
the first line (`openremote-agy/src/driver.rs:123-130`). Keeping the pipe open
would give Antigravity a persistent, steerable session with a real interrupt.

That is a protocol change, not a flag, so it belongs with G10 rather than in a
one-line fix.

### G16. Pi's RPC surface is mostly unused

Pi 1.4.0's `docs/rpc-commands.md` lists 33 commands. The driver uses `get_state`,
`prompt`, `abort`, `set_model`, `set_thinking_level` and `get_commands`. Unused:
`get_available_models`, `get_available_thinking_levels`, `get_session_stats`,
`compact`, `set_auto_compaction`, `cycle_model`, `switch_session`, `fork`,
`clone`, `steer`, `follow_up`, `clear_queue`, `new_session`, `set_steering_mode`,
`set_follow_up_mode`, `export_html`.

Every row of G9, G10 and G11 for Pi is one of these calls. Pi is the App with
the most wire and the least driver.

### G17. Antigravity shows no tool activity at all

The driver reads `step_update.text_delta` and nothing else
(`openremote-agy/src/driver.rs:288-301`); every other event type falls into
`_ => {}`. There is no tool card, no reasoning, no usage, no model report. Six
of seventeen events in total, the lowest of the six. A user watching
Antigravity work sees text appear and nothing else.

### G18. Grok's tool cards are half a story

Grok emits `ToolResult` with an empty `tool_use_id`, the name `commandExecution`
for every tool, and no `ToolStarted` at all
(`openremote-grok/src/driver.rs:393-403`). The wire parser has
`tool_use_blocks` ready (`anthropic_wire.rs:84-95`) and the driver never calls
it, so a Grok turn shows results floating without the calls that produced them,
all labelled as command executions even when they were file edits.

---

## 6. The contract and the plugin seam

### G19. The plugin protocol is a subset, and the asymmetry is invisible

`docs/harness-plugins.md` documents these events: `initialized`, `text`,
`text_delta`, `reasoning`, `reasoning_delta`, `tool_started`, `tool_result`,
`approval_requested`, `turn_completed`, `context_used`, `turn_cost`, `stderr`,
`closed` (`:110-126`). The daemon's enum has four more that no plugin can send:
`CommandsListed`, `ModelReported`, `ThinkingTokens`, `Compacted`.

The spawn command carries `cwd`, `model`, `permission_mode`, `effort`, `resume`
and `fast` (`plugin_driver.rs:165-173`). `SpawnOptions` has five more that never
cross the seam: `endpoint`, `include_deltas`, `mcp_servers`, `plugin_dirs`,
`allowed_tools`.

A plugin that wants compaction markers, a refreshed command list or MCP servers
has no way to say so, and a user has no way to tell which plugin speaks which
subset. This is the same shape as G3, one layer out.

### G20. A driver cannot advertise what it honors

There is no capability object on `Harness`. The console infers capability from
absence: no `permission_modes` means no picker, `fast_supported` false means no
fast chip, `reasoning_efforts` empty means no effort picker. That works when a
harness has no such concept (Antigravity's effort in the model id) and fails when
the driver simply does not implement it (Grok's effort, G1). The two are
indistinguishable to every reader.

**Fix:** fields on `Harness` - `honors: {effort, fast, mcp, plugins, tools,
attachments}` - filled per driver, and a console that dims a control instead of
silently accepting it. That is also what makes G3 and G4 shippable honestly.

---

## 7. What each App is like today

- **Claude Code.** The reference. Sixteen of seventeen events, approvals with a
  question wire, plugins, MCP, endpoints, live model and effort. Missing: cost
  on this transport (unverified), the context window, `allowed_tools` (G3), the
  plan body in plan-mode approvals, subagent nesting (`parent_tool_use_id` is
  dropped), images (G9).
- **Codex.** Second best. Approvals with a session grant, steering, context
  window, compaction, reasoning, MCP, effort, fast. Missing: images, a sandbox
  choice (G13), plugins, cost, the command list as a live event.
- **Grok.** A working print-mode driver with a real permission vocabulary and a
  good model catalog, and two dead branches (G1, G2). No approvals by design
  (the limitation is documented in `openremote-grok/src/lib.rs:11-14`), no tool
  calls (G18), no context, no commands.
- **Pi.** The most wire and the least use of it (G16). Persistent, steerable in
  principle, with a full RPC; in practice the driver uses six calls. No model
  picker and no effort picker until a provider is signed in (matrix note 7), and
  it writes into the workspace (G7).
- **OpenCode.** The richest approval model (once, always, reject, plus questions
  and forms), deltas, tool cards. No cost, context, compaction, MCP or
  permission words, and the v1 defects in G8.
- **Antigravity.** Six of seventeen events (G17). Model picker, streaming text,
  resume, an execution-mode wire nobody can reach (G4). No tools, usage,
  approvals or commands.

---

## 8. Drift and hygiene

- **G21. The audit that is cited 65 times is not in the tree.** Comments across
  17 files reference findings from an unnamed "harness audit" - thirteen distinct
  numbers, `F2` through `F18` - and no such document exists in the repository.
  The project's own convention says comments must not reference artifacts that
  are no longer in the tree (`AGENTS.md:150-152`). Either the audit lands in
  `docs/` or the references become prose.
- **G22. Three stale doc claims.** `openremote-grok/src/lib.rs:3-4` documents
  `--single` and "verified live against grok 1.0.67"; the driver ships
  `--prompt-file` and the installed CLI is 1.0.70. `openremote-agy/src/driver.rs:187-188`
  cites 1.4.7 and the installed CLI is 1.4.10. `openremote-claude/src/driver.rs:210`
  says "SIGTERM, wait up to 5s, SIGKILL" and no SIGTERM is ever sent
  (`openremote-harness/src/process.rs:71`).
- **G23. The endpoint harness list disagrees with itself.** `http.rs:374` and
  `model.rs:220` say claude, codex and grok; `registry.rs:637-639` says claude.
  See G5.

---

## 9. What is already right, and should not be traded away

- **Prompts never ride argv.** Grok writes the prompt to a fresh 0600 temp file
  and passes `--prompt-file`; Antigravity sends it as one JSON line on stdin;
  every chosen word goes through `validate_argv_word`. The tests for this are
  good (`openremote-grok/src/driver.rs:519-563`).
- **Every turn boundary is the harness's own frame.** The only synthesized
  boundaries are the three documented EOF cases, and no driver ever synthesizes
  a success.
- **Approvals speak the harness's words.** `accept | acceptForSession | decline
  | cancel` for Codex, `once | always | reject` for OpenCode, `allow | deny` for
  Claude. Nothing is translated into an OpenRemote dialect.
- **Bounded, lossy reads** (`harness audit F11` behavior) in the five crates
  that read NDJSON: an over-long line is dropped, not buffered until the daemon
  dies. OpenCode is the exception, because it reads its event stream over HTTP
  instead of stdio.
- **Honest refusals exist and are used.** Custom endpoints, live model changes
  on Grok and Antigravity, and Pi's fast mode all refuse with a sentence rather
  than failing quietly. G3 and G8 are gaps precisely because they are the places
  that ignore instead of refuse.

---

## 10. Order of work

Cheapest first, sized by one person.

| # | Work | Size | Why first |
|---|---|---|---|
| 1 | G1, G2: Grok effort and `--include-partial-messages` | 1 day | Two dead controls on a shipped App; both are one line plus a fixture |
| 2 | G4: Antigravity permission words in the registry | hours | Driver is done; this also closes the invisible-word path |
| 3 | G3: refuse, don't ignore, the fields a driver cannot honor, and dim the MCP row per App | 1-2 days | Stops the console claiming MCP and plugins on four Apps |
| 4 | G6: plugin `answer` | 1 day | Latent, cheap, and the contract already promises it |
| 5 | G19 + G20: extend the plugin protocol and add a capability object | 3-4 days | Turns every later item into a visible, honest control |
| 6 | G5, G23: one endpoint harness list | hours | Removes a record a user can create and never use |
| 7 | G11, G12: context, cost, usage and Grok's command list | 3-5 days | Context and cost reach four more Apps; Grok gets its slash commands |
| 8 | G9: attachments, contract through composer | 1-2 weeks | The largest reachable win; blocked today by one missing field |
| 9 | G10, G15, G16: steering on Pi and OpenCode, persistent Antigravity, the unused Pi RPC | 2-3 weeks | Makes the persistent Apps feel persistent |
| 10 | G13, G14: sandbox choice per chat, and an ACP evaluation | 1-2 weeks | The product-level half, better argued in the gap map (A1, A2, E9) |

Items 1-6 are where the console currently promises something the build does not
do. Items 7-10 are where the Apps stop being unequal.

---

## Appendix A: how to re-check a claim

```sh
cd daemon/crates

# which spawn options a driver actually reads
rg -n 'opts\.(effort|fast|mcp_servers|plugin_dirs|allowed_tools|endpoint|include_deltas)' openremote-*/src

# what each App advertises
rg -n 'permission_modes|fast_supported|custom_endpoint_supported|default_effort' openremote-daemon/src/registry.rs

# the plugin seam's spawn fields and the answer refusal
rg -n '"type": "spawn"|asks for no approvals' openremote-daemon/src/plugin_driver.rs
```

## Appendix B: what was not verified

- No chat was executed against a signed-in model. Claims about whether a CLI
  emits a frame in practice (Claude's cost field, Grok's deltas with the flag,
  Antigravity's `usage`) are marked unverified above.
- Claude Code's CLI is absent from this machine; its "offered but unused"
  claims come from the driver and from published SDK behavior, not from a local
  `--help`.
- The OpenCode v1 reply-path defect (G8) is read from code; no v1 server was run
  against it.
- G7's "a moved chat cannot reach its transcript" is an inference from a
  relative `--session-dir`, not an exercised failure.

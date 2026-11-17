# Every App's slash commands in the composer

The user asked for "every single slash command the harnesses have, with
proper support for everything and hot-reload", plus any improvements worth
making. They handed the design calls over ("any improvements you think is
good"), so the choices below are the builder's calls, written out so they
can be checked. The per-App inventory this design rests on is
`docs/slash-commands.md`.

## Where things stand (the branch this starts from)

- `GET /commands` (`supervisor/composer.rs`) already asks each App for its
  list, per workspace, cached under a stamp of the App's version and the
  installed plugins: Claude Code through its `initialize` handshake, Codex
  through `skills/list`, OpenCode through `GET /command`, pi through
  `get_commands`, a plugin's App through `--commands`. grok and
  Antigravity list nothing.
- The composer (`composer-menus.js`, `useComposerMenus.jsx`) shows
  OpenRemote's own commands (`/model`, `/effort`, `/fast`,
  `/permissions`, `/stop`, `/resume`, `/rename`, `/sync`, `/cloud`), a
  Recent group kept on the device, a few everyday ones, and
  prefix/substring search over the App's list, split into Commands and
  Skills.
- Anything else typed after `/` is sent as text. Claude Code runs most of
  its own commands that way; Codex, pi and OpenCode run only their custom
  commands or skills that way and pass a built-in such as `/compact` to
  the model as words.
- The list is only asked again when a plugin changes, when a running
  Claude chat reports a name the cache lacks, or when the composer
  remounts. A new `.claude/commands/x.md` does not show until then.

## Goals

1. Every command each App offers is in the `/` menu: its built-ins, the
   user's and the project's custom commands, skills and prompts, plugin
   commands, and MCP prompts where the App exposes them as commands - with
   a description and an argument hint.
2. A picked command does what it does in the terminal, by the route that
   works for that App: sent as text, run through the App's own protocol
   call, run by OpenRemote itself, or refused with a sentence that says
   why.
3. A command whose terminal version is a picker or a panel has a matching
   piece of console UI (a choice menu, a view, a dialog), never a dead
   end.
4. Hot reload: adding, editing or removing a command, skill, prompt,
   plugin or MCP server shows in the menu within about a second, with no
   chat or daemon restart.

## Non-goals

- Re-implementing an App's terminal UI. Where the App draws a full TUI
  panel (`/config`, `/agents` editor, `/memory` editor), OpenRemote opens
  its own nearest view or refuses; it does not clone the panel.
- Changing what an App's command does. The App's vocabulary stays the
  App's (AGENTS.md conventions).
- Fixing the Codex `on-failure` approval word (a known separate bug).

## Anti-goals

- A menu of a hundred rows before typing. The current shape (OpenRemote,
  Recent, a few everyday, "type to search N") stays.
- A command that silently becomes model input. Sending `/compact` to a
  model as words is the failure this design exists to end: a built-in the
  App cannot run headless is refused, not forwarded.
- Stale rows: a command shown that the App no longer has.

## The design

### One catalog, built in the daemon

The daemon owns the list, so the desktop, the web console, a phone and an
automation all see and get the same answer. Each `CommandEntry` gains:

- `source`: `builtin` (the App's own, from OpenRemote's table),
  `harness` (listed by the App at run time: custom commands, skills,
  prompts), `plugin`, `mcp` (an MCP prompt), as today plus the two new
  words.
- `route`: how it runs.
  - `native` - sent as the message text; the App runs it.
  - `driver` - OpenRemote calls the App's protocol for it (Codex
    `thread/compact/start`, pi's `compact` frame, OpenCode's
    `summarize`). The chat shows the command as the user typed it.
  - `console` with an `action` word - OpenRemote does it (`new-chat`,
    `stop`, `open-plugins`, `open-mcp`, `open-apps`, `changes`,
    `copy-last`, `status`, `help`, and the existing model, effort, fast,
    permission and rename actions).
  - `refuse` with `why` - shown dimmed in the menu with its reason, and
    refused at send.
- `args`: for a command whose argument is a fixed choice (`/model`,
  `/permissions`, `/effort`, `/approvals`), the word that names its
  choice list, which the composer's argument menu already fills.

The built-in table is static Rust, one per App, in a new module
`daemon/crates/openremote-daemon/src/slash.rs`, written from the
inventory. A run-time list wins over the table for a name both have
(the App's own description is the truer one), but the table's `route`
stays: Claude Code lists `/compact` and the table says `native`.

The daemon is also the net: `prompt`, `queue` and `steer` look up the
message's leading command and refuse a `refuse` route, and a `console`
route that arrived as text (a phone or an automation sending `/clear`),
with a 409 whose sentence is the row's `why`, before any turn opens. A
`driver` route runs the App's call inside the normal turn, so the chat's
status and transcript read like any other message.

### The composer

- Rows carry the route: a `refuse` row is dimmed with its reason (it
  stays findable, which is how the user learns it exists and why it does
  not work here); a `console` row runs in the app at send; `native` and
  `driver` rows go to the daemon.
- Per-App badge on rows that are not the App's built-ins: `skill`,
  `plugin` (with the plugin's name), `MCP`, `custom` (the user's or the
  project's own), so the user can tell `/review` from their own
  `/review-pr`.
- Fuzzy search: a subsequence match after the existing prefix and
  substring tiers, with aliases, so `/sr` finds `/security-review`.
- Recent first, per App: the Recent group is kept per App id, since
  `/compact` used on Codex says nothing about pi.
- Argument hint: shown in the row and, once a command is picked, as
  ghost text after the caret until the user types an argument.
- `/help` lists everything the menu would, grouped, in a dialog
  (`useDialog`), searchable - the terminal's `/help` screen.
- Errors from a refused or failed command go through `errorCopy`.

### Hot reload

- A watcher in the daemon (the `notify` crate, already in the lockfile's
  cache) watches each folder a command can come from, for every
  workspace the composer has asked about and the user's home folders:
  `.claude/commands`, `.claude/skills`, `.claude/agents`,
  `~/.claude/commands|skills|agents`, `~/.codex/prompts`,
  `~/.codex/skills`, `.codex/skills`, `.agents/skills`,
  `.opencode/command(s)`, `~/.config/opencode/command(s)`,
  `opencode.json`, `.pi/prompts`, `~/.pi/agent/prompts|skills`, the
  plugin folder, and the MCP settings. A folder that does not exist yet
  is watched through its nearest existing parent, so the first file in a
  new `.claude/commands` counts.
- Events are debounced (300 ms quiet). Then every cache entry whose
  workspace or home folder the change touched is marked stale, and a
  daemon-wide `commands_rev` counter goes up.
- Harness-reported lists stay a second signal: a running chat whose init
  names a command the cache lacks marks it stale (today's rule), and now
  also bumps `commands_rev`.
- `commands_rev` rides `/capabilities`, which the console's beat already
  reads every 2.5 s; `/commands` answers carry it too. The composer asks
  again when the rev moves, and whenever the `/` menu opens on a list
  older than the current rev - so a remote machine's change shows the
  next time the menu opens there.
- While a new list is asked, the previous one stays on screen (no
  flicker), the same rule the file list already follows.

### Improvements chosen

Per-App badges, fuzzy search, per-App recents, argument ghost hints, a
`/help` dialog, the refusal sentence for terminal-only commands, and the
daemon-side refusal so a phone or automation cannot push a built-in into
the model as words.

## Constraints

- Errors the user reads go through `errorCopy`; dialogs use `useDialog`;
  a new Settings row is registered in `settings-search.js` (none is
  planned); the user-facing word is "App".
- No em dashes in any text.
- The watcher must not hold a folder open in a way that stops the user
  deleting or renaming it on Windows (watch parents non-recursively
  where possible; drop watches for workspaces not asked about for 30
  minutes).
- Tests: daemon unit tests for the table, the route lookup, the refusal,
  the debounce and the stale marking; an e2e that writes a command file
  and sees `/commands` and `commands_rev` change without a restart;
  console tests for the fuzzy rank, per-App recents, badges, refusal and
  console actions.

## Rules settled after the builder check (builder's calls)

- **A custom command with a built-in's name.** Claude Code's own list
  holds its built-ins and the user's commands together, and Claude runs
  its built-in first, so for Claude the table's route stands. Codex,
  OpenCode and pi list only custom entries at run time, so a run-time
  name on those Apps is the user's own and is always `native`, even where
  the table has a built-in of that name. Codex custom prompts are
  `/prompts:<name>` and never clash.
- **A `console` command that reaches the daemon as text** (a phone, an
  automation). Each table row says what happens then: `fallback: native`
  when the App runs the word itself (Claude's `/model x`, `/clear`,
  `/rename`), so what works today keeps working; otherwise it is refused
  with the row's `why`. Only rows with no native fallback are refused.
- **A `driver` or `refuse` command mid-turn.** Queued messages are sent
  through the normal prompt path when the turn ends, so a queued
  `/compact` runs then. A steer whose text is a `driver`, `console` or
  `refuse` command is refused ("send it once the turn has finished"),
  because folding it into a running turn would send it as words.
- **Another machine's list.** The composer asks again each time the `/`
  menu opens, which is cheap (the daemon's cache answers) and needs no
  new peer route. `commands_rev` on the local `/capabilities` covers this
  computer's chats while the menu is open.
- **Plugin Apps, grok and Antigravity.** A plugin App has no table: its
  `--commands` entries are `native`, and the plugin contract does not
  change. grok and Antigravity get tables from the inventory; what their
  CLIs list at run time, if anything, is `native`.
- **MCP.** Claude reports MCP prompts in its own list as
  `mcp__<server>__<prompt>`; those rows get `source: mcp`. The watcher
  covers `.mcp.json` in the workspace, `~/.codex/config.toml`,
  `opencode.json(c)` in the workspace and `~/.config/opencode/`, and
  OpenRemote's own MCP and plugin changes bump `commands_rev` directly.
  `~/.claude.json` is not watched: Claude rewrites it all the time, so
  watching it would mean asking again constantly.

## Left to the builder

- The exact wording of each refusal and description (short, the App's
  own words where it has them).
- Which Codex, pi and OpenCode built-ins get a `driver` route in this
  pass, based on what each protocol actually offers; anything without a
  call is `refuse` with a reason.
- Visual details of the badges and the ghost hint, within the existing
  composer styles.
